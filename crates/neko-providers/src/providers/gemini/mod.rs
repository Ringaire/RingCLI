/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> crate::catalog::CatalogEntry {
    crate::catalog::CatalogEntry {
        name: "Google Gemini".into(),
        kind: crate::catalog::ProviderKind::Gemini,
        base_url: Some("https://generativelanguage.googleapis.com".into()),
        api_key_env: Some("GEMINI_API_KEY".into()),
        default_model: Some("gemini-2.0-flash".into()),
        extra_body: None,
    }
}

use async_trait::async_trait;
use neko_core::tools::{ContentBlock, Message, MessageRole};
use reqwest::Client;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::error::ProviderError;
use crate::provider::{
    check_response_error, ChatRequest, ChatResponse, ModelInfo, Provider, ProviderStream,
    StopReason, StreamChunk, StreamEvent, ToolDef, Usage,
};

const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_MODEL: &str = "gemini-2.0-flash";

fn convert_messages_to_contents(msgs: &[Message]) -> Value {
    let mut contents = Vec::new();
    for msg in msgs {
        let role = match msg.role {
            MessageRole::User | MessageRole::ToolResult => "user",
            MessageRole::Assistant => "model",
        };
        let parts: Vec<Value> = msg
            .content
            .iter()
            .filter_map(|blk| match blk {
                ContentBlock::Text { text } => Some(json!({ "text": text })),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            contents.push(json!({ "role": role, "parts": parts }));
        }
    }
    Value::Array(contents)
}

fn convert_tools(tools: &[ToolDef]) -> Value {
    let function_declarations: Vec<Value> = tools
        .iter()
        .map(|t| json!({
            "name": t.name,
            "description": t.description,
            "parameters": t.input_schema,
        }))
        .collect();
    json!([{ "function_declarations": function_declarations }])
}

fn parse_finish_reason(s: Option<&str>) -> StopReason {
    match s {
        Some("STOP")           => StopReason::EndTurn,
        Some("MAX_TOKENS")     => StopReason::MaxTokens,
        _                      => StopReason::EndTurn,
    }
}

pub struct GeminiProvider {
    client:    Client,
    api_key:   String,
    base_url:  String,
    def_model: String,
}

impl GeminiProvider {
    pub fn new(api_key: impl Into<String>, base_url: Option<String>, def_model: Option<String>) -> Self {
        let client = crate::provider::build_http_client(None, CONNECT_TIMEOUT_SECS);
        Self::with_client(client, api_key, base_url, def_model)
    }

    pub fn with_client(
        client:    Client,
        api_key:   impl Into<String>,
        base_url:  Option<String>,
        def_model: Option<String>,
    ) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.unwrap_or_else(|| GEMINI_BASE_URL.to_string()),
            def_model: def_model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    fn endpoint(&self, model: &str, stream: bool) -> String {
        let action = if stream { "streamGenerateContent" } else { "generateContent" };
        format!(
            "{}/v1beta/models/{}:{}?key={}",
            self.base_url, model, action, self.api_key
        )
    }

    fn build_body(&self, req: &ChatRequest) -> Value {
        let mut body = json!({
            "contents": convert_messages_to_contents(&req.messages),
            "generationConfig": {
                "maxOutputTokens": req.max_tokens,
            },
        });
        if let Some(sys) = &req.system {
            body["systemInstruction"] = json!({ "parts": [{ "text": sys }] });
        }
        if !req.tools.is_empty() {
            body["tools"] = convert_tools(&req.tools);
        }
        if let Some(t) = req.temperature {
            body["generationConfig"]["temperature"] = json!(t);
        }
        body
    }

}

#[async_trait]
impl Provider for GeminiProvider {
    fn id(&self) -> &str { "gemini" }
    fn display_name(&self) -> &str { "Google Gemini" }
    fn default_model(&self) -> &str { &self.def_model }

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError> {
        let url  = self.endpoint(&req.model, false);
        let body = self.build_body(req);
        debug!(model = %req.model, "gemini chat request");

        let resp = tokio::select! {
            r = self.client.post(&url).json(&body).send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;
        let raw: Value = resp.json().await.map_err(ProviderError::Network)?;

        let candidate = raw["candidates"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| ProviderError::Other("no candidates in response".into()))?;

        let finish_reason = candidate["finishReason"].as_str();
        let stop_reason   = parse_finish_reason(finish_reason);

        let usage = Usage {
            input_tokens:          raw["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0),
            output_tokens:         raw["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0),
            cache_creation_tokens: 0,
            cache_read_tokens:     0,
        };

        let mut content_blocks = Vec::new();
        if let Some(parts) = candidate["content"]["parts"].as_array() {
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    content_blocks.push(ContentBlock::Text { text: text.to_string() });
                }
                if let Some(fc) = part.get("functionCall") {
                    let name  = fc["name"].as_str().unwrap_or("").to_string();
                    let input = fc["args"].clone();
                    content_blocks.push(ContentBlock::ToolUse {
                        tool_use_id: uuid::Uuid::new_v4().to_string(),
                        tool_name:   name,
                        tool_input:  input,
                    });
                }
            }
        }

        let message = Message::new(MessageRole::Assistant, content_blocks);
        Ok(ChatResponse { message, stop_reason, usage, model: req.model.clone() })
    }

    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError> {
        let url  = self.endpoint(&req.model, true);
        let body = self.build_body(req);

        let resp = tokio::select! {
            r = self.client.post(&url).header("Accept", "text/event-stream").json(&body).send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let byte_stream = resp.bytes_stream();
        tokio::spawn(run_gemini_sse(byte_stream, signal, tx));
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(gemini_known_models())
    }
}

async fn run_gemini_sse<S>(
    byte_stream: S,
    signal:      CancellationToken,
    tx:          tokio::sync::mpsc::Sender<StreamEvent>,
) where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    tokio::pin!(byte_stream);
    let mut buf = String::new();

    loop {
        tokio::select! {
            _ = signal.cancelled() => {
                let _ = tx.send(StreamEvent::Error("cancelled".into())).await;
                return;
            }
            chunk = futures_util::StreamExt::next(&mut byte_stream) => {
                let bytes = match chunk {
                    None => break,
                    Some(Err(e)) => { let _ = tx.send(StreamEvent::Error(e.to_string())).await; return; }
                    Some(Ok(b)) => b,
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(end) = buf.find("\n\n") {
                    let block = buf[..end].to_string();
                    buf = buf[end + 2..].to_string();

                    let data = block.lines()
                        .find_map(|l| l.strip_prefix("data: ").map(str::to_string));
                    let data = match data { Some(d) => d, None => continue };

                    let raw: Value = match serde_json::from_str(&data) { Ok(v) => v, Err(_) => continue };
                    let candidate = match raw["candidates"].as_array().and_then(|a| a.first()) {
                        Some(c) => c.clone(),
                        None => continue,
                    };
                    if let Some(parts) = candidate["content"]["parts"].as_array() {
                        for part in parts {
                            if let Some(text) = part["text"].as_str() {
                                let _ = tx.send(StreamEvent::Chunk(StreamChunk::TextDelta { delta: text.to_string() })).await;
                            }
                        }
                    }
                }
            }
        }
    }
    let _ = tx.send(StreamEvent::Done { stop_reason: StopReason::EndTurn, usage: Usage::default() }).await;
}

fn gemini_known_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gemini-2.0-flash".into(),
            display_name: "Gemini 2.0 Flash".into(),
            context_window: 1_000_000,
            max_output_tokens: 8_192,
            supports_vision: true,
            supports_thinking: false,
            supports_tools: true,
        },
        ModelInfo {
            id: "gemini-2.5-pro".into(),
            display_name: "Gemini 2.5 Pro".into(),
            context_window: 1_000_000,
            max_output_tokens: 65_536,
            supports_vision: true,
            supports_thinking: true,
            supports_tools: true,
        },
    ]
}
