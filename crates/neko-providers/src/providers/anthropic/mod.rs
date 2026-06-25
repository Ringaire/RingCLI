pub mod claude_code;

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> crate::catalog::CatalogEntry {
    crate::catalog::CatalogEntry {
        name: "Anthropic".into(),
        kind: crate::catalog::ProviderKind::Anthropic,
        base_url: Some("https://api.anthropic.com".into()),
        api_key_env: Some("ANTHROPIC_API_KEY".into()),
        default_model: Some("claude-sonnet-4-6".into()),
        extra_body: None,
    }
}

use async_trait::async_trait;
use neko_core::tools::{ContentBlock, Message, MessageRole};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::error::ProviderError;
use crate::provider::{
    check_response_error, ChatRequest, ChatResponse, ModelInfo, Provider, ProviderStream,
    StopReason, StreamChunk, StreamEvent, ToolDef, Usage, DEFAULT_CONTEXT_WINDOW,
    DEFAULT_THINKING_BUDGET,
};

// ── API 常量 ──────────────────────────────────────────────────────────────────

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const ANTHROPIC_BETA_TOOLS: &str = "tools-2024-04-04";
const ANTHROPIC_BETA_THINKING: &str = "interleaved-thinking-2025-05-14";
const CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_MODEL: &str = "claude-opus-4-5";

// ── 响应结构（反序列化）──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens:               Option<u64>,
    output_tokens:              Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens:    Option<u64>,
}

impl From<AnthropicUsage> for Usage {
    fn from(u: AnthropicUsage) -> Self {
        Self {
            input_tokens:          u.input_tokens.unwrap_or(0),
            output_tokens:         u.output_tokens.unwrap_or(0),
            cache_creation_tokens: u.cache_creation_input_tokens.unwrap_or(0),
            cache_read_tokens:     u.cache_read_input_tokens.unwrap_or(0),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text:  Option<String>,
    id:    Option<String>,
    name:  Option<String>,
    input: Option<Value>,
    /// extended thinking 块内容；反序列化保留，当前不单独渲染
    #[allow(dead_code)]
    thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessage {
    stop_reason: Option<String>,
    content: Vec<AnthropicContentBlock>,
    usage: Option<AnthropicUsage>,
    model: Option<String>,
}

// ── SSE 事件解析 ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SseEvent {
    MessageStart { message: AnthropicMessage },
    ContentBlockStart { index: usize, content_block: AnthropicContentBlock },
    ContentBlockDelta { index: usize, delta: SseDelta },
    ContentBlockStop { index: usize },
    MessageDelta { delta: SseMessageDelta, usage: Option<AnthropicUsage> },
    MessageStop,
    Error { error: Value },
    Ping,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)] // 变体名对应 Anthropic API 的 delta 类型，保持一致
enum SseDelta {
    TextDelta { text: String },
    ThinkingDelta { thinking: String },
    InputJsonDelta { partial_json: String },
    SignatureDelta {
        #[allow(dead_code)]
        signature: String,
    },
}

#[derive(Debug, Deserialize)]
struct SseMessageDelta {
    stop_reason: Option<String>,
}

// ── 消息格式转换 ──────────────────────────────────────────────────────────────

fn convert_messages(msgs: &[Message]) -> Value {
    let mut out = Vec::new();
    for msg in msgs {
        let role = match msg.role {
            MessageRole::User | MessageRole::ToolResult => "user",
            MessageRole::Assistant => "assistant",
        };
        let content: Vec<Value> = msg
            .content
            .iter()
            .map(|blk| match blk {
                ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                ContentBlock::ToolUse { tool_use_id, tool_name, tool_input } => json!({
                    "type": "tool_use",
                    "id": tool_use_id,
                    "name": tool_name,
                    "input": tool_input,
                }),
                ContentBlock::ToolResult { tool_use_id, tool_result, is_error } => json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": tool_result.to_string(),
                    "is_error": is_error,
                }),
            })
            .collect();
        out.push(json!({ "role": role, "content": content }));
    }
    Value::Array(out)
}

fn convert_tools(tools: &[ToolDef]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            }))
            .collect(),
    )
}

fn parse_stop_reason(s: Option<&str>) -> StopReason {
    match s {
        Some("end_turn")       => StopReason::EndTurn,
        Some("tool_use")       => StopReason::ToolUse,
        Some("max_tokens")     => StopReason::MaxTokens,
        Some("stop_sequence")  => StopReason::StopSequence,
        _                      => StopReason::EndTurn,
    }
}

// ── AnthropicProvider ─────────────────────────────────────────────────────────

pub struct AnthropicProvider {
    client:   Client,
    api_key:  String,
    base_url: String,
}

impl AnthropicProvider {
    /// 使用独立 client（无代理）。便捷构造。
    pub fn new(api_key: impl Into<String>, base_url: Option<String>) -> Self {
        let client = crate::provider::build_http_client(None, CONNECT_TIMEOUT_SECS);
        Self::with_client(client, api_key, base_url)
    }

    /// 使用调用方提供的共享 client（可携带代理配置）。
    pub fn with_client(client: Client, api_key: impl Into<String>, base_url: Option<String>) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.unwrap_or_else(|| ANTHROPIC_BASE_URL.to_string()),
        }
    }

    /// 从 `GET /v1/models` 拉取模型列表。能力字段由 `infer_caps` 按 id 推断，
    /// 若响应带 `display_name` 则覆盖默认值。
    async fn fetch_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = format!("{}/v1/models?limit=1000", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Http { status: status.as_u16(), body });
        }

        let parsed: ModelsListResponse = resp.json().await?;
        let models = parsed
            .data
            .into_iter()
            .map(|m| {
                let mut info = infer_caps(&m.id);
                if let Some(name) = m.display_name {
                    info.display_name = name;
                }
                info
            })
            .collect();
        Ok(models)
    }

    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model":      req.model,
            "messages":   convert_messages(&req.messages),
            "max_tokens": req.max_tokens,
            "stream":     stream,
        });
        if let Some(sys) = &req.system {
            body["system"] = Value::String(sys.clone());
        }
        if !req.tools.is_empty() {
            body["tools"] = convert_tools(&req.tools);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(p) = req.top_p {
            body["top_p"] = json!(p);
        }
        if !req.stop.is_empty() {
            body["stop_sequences"] = json!(req.stop);
        }
        if req.extended_thinking {
            let budget = req.thinking_budget.unwrap_or(DEFAULT_THINKING_BUDGET);
            body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
        }
        body
    }

    fn build_betas(&self, req: &ChatRequest) -> String {
        let mut betas = vec![ANTHROPIC_BETA_TOOLS];
        if req.extended_thinking {
            betas.push(ANTHROPIC_BETA_THINKING);
        }
        betas.join(",")
    }

}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str { "anthropic" }
    fn display_name(&self) -> &str { "Anthropic" }
    fn default_model(&self) -> &str { DEFAULT_MODEL }

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError> {
        let url  = format!("{}/v1/messages", self.base_url);
        let body = self.build_body(req, false);
        let betas = self.build_betas(req);

        debug!(model = %req.model, "anthropic chat request");

        let http_req = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("anthropic-beta", &betas)
            .json(&body);

        let resp = tokio::select! {
            r = http_req.send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;
        let msg: AnthropicMessage = resp.json().await.map_err(ProviderError::Network)?;

        let stop_reason = parse_stop_reason(msg.stop_reason.as_deref());
        let usage = msg.usage.map(Usage::from).unwrap_or_default();
        let model = msg.model.unwrap_or_else(|| req.model.clone());

        let mut content_blocks = Vec::new();
        for blk in &msg.content {
            match blk.kind.as_str() {
                "text" => {
                    if let Some(text) = &blk.text {
                        content_blocks.push(ContentBlock::Text { text: text.clone() });
                    }
                }
                "tool_use" => {
                    if let (Some(id), Some(name)) = (&blk.id, &blk.name) {
                        content_blocks.push(ContentBlock::ToolUse {
                            tool_use_id: id.clone(),
                            tool_name: name.clone(),
                            tool_input: blk.input.clone().unwrap_or(Value::Object(Default::default())),
                        });
                    }
                }
                _ => {}
            }
        }

        let message = Message::new(MessageRole::Assistant, content_blocks);
        Ok(ChatResponse { message, stop_reason, usage, model })
    }

    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError> {
        let url  = format!("{}/v1/messages", self.base_url);
        let body = self.build_body(req, true);
        let betas = self.build_betas(req);

        debug!(model = %req.model, "anthropic stream request");

        let http_req = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("anthropic-beta", &betas)
            .header("Accept", "text/event-stream")
            .json(&body);

        let resp = tokio::select! {
            r = http_req.send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let byte_stream = resp.bytes_stream();
        tokio::spawn(run_anthropic_sse(byte_stream, signal, tx));
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        // 优先实时拉取；任何失败（无网络/无 key/限流）都优雅降级到默认模型，
        // 不让模型目录因一次网络抖动而为空。
        match self.fetch_models().await {
            Ok(models) if !models.is_empty() => Ok(models),
            Ok(_) => Ok(vec![fallback_model_info()]),
            Err(e) => {
                warn!(error = %e, "anthropic: list_models fetch failed, using fallback");
                Ok(vec![fallback_model_info()])
            }
        }
    }
}

// ── SSE 流解析（后台任务版）──────────────────────────────────────────────────

async fn run_anthropic_sse<S>(
    byte_stream: S,
    signal:      CancellationToken,
    tx:          tokio::sync::mpsc::Sender<StreamEvent>,
) where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    tokio::pin!(byte_stream);

    let mut buf         = String::new();
    let mut tool_inputs: std::collections::HashMap<usize, String> = Default::default();
    let mut tool_ids:    std::collections::HashMap<usize, String> = Default::default();
    let mut usage       = Usage::default();
    let mut stop_reason = None::<StopReason>;

    loop {
        tokio::select! {
            _ = signal.cancelled() => {
                let _ = tx.send(StreamEvent::Error("cancelled".into())).await;
                return;
            }
            chunk = futures_util::StreamExt::next(&mut byte_stream) => {
                let bytes = match chunk {
                    None => break,
                    Some(Err(e)) => {
                        let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                        return;
                    }
                    Some(Ok(b)) => b,
                };

                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(block_end) = buf.find("\n\n") {
                    let block = buf[..block_end].to_string();
                    buf = buf[block_end + 2..].to_string();

                    let data = block.lines()
                        .find_map(|l| l.strip_prefix("data: ").map(str::to_string));
                    let data = match data { Some(d) => d, None => continue };
                    if data == "[DONE]" { continue; }

                    let sse: SseEvent = match serde_json::from_str(&data) {
                        Ok(e) => e,
                        Err(e) => { warn!(err = %e, "SSE parse error"); continue; }
                    };

                    match sse {
                        SseEvent::MessageStart { message } => {
                            if let Some(u) = message.usage {
                                usage = usage.clone() + Usage::from(u);
                            }
                        }
                        SseEvent::ContentBlockStart { index, content_block } => {
                            if content_block.kind == "tool_use" {
                                if let (Some(id), Some(name)) = (content_block.id, content_block.name) {
                                    let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallStart {
                                        call_id: id.clone(), tool_name: name,
                                    })).await;
                                    tool_ids.insert(index, id);
                                    tool_inputs.insert(index, String::new());
                                }
                            }
                        }
                        SseEvent::ContentBlockDelta { index, delta } => {
                            match delta {
                                SseDelta::TextDelta { text } => {
                                    let _ = tx.send(StreamEvent::Chunk(StreamChunk::TextDelta { delta: text })).await;
                                }
                                SseDelta::ThinkingDelta { thinking } => {
                                    let _ = tx.send(StreamEvent::Chunk(StreamChunk::ThinkingDelta { delta: thinking })).await;
                                }
                                SseDelta::InputJsonDelta { partial_json } => {
                                    if let Some(call_id) = tool_ids.get(&index).cloned() {
                                        if let Some(acc) = tool_inputs.get_mut(&index) { acc.push_str(&partial_json); }
                                        let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallInput { call_id, delta: partial_json })).await;
                                    }
                                }
                                SseDelta::SignatureDelta { .. } => {}
                            }
                        }
                        SseEvent::ContentBlockStop { index } => {
                            if let Some(call_id) = tool_ids.get(&index).cloned() {
                                let raw = tool_inputs.get(&index).cloned().unwrap_or_default();
                                let full_input: Value = serde_json::from_str(&raw)
                                    .unwrap_or(Value::Object(Default::default()));
                                let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallDone { call_id, full_input })).await;
                            }
                        }
                        SseEvent::MessageDelta { delta, usage: u } => {
                            if let Some(u) = u { usage = usage.clone() + Usage::from(u); }
                            stop_reason = Some(parse_stop_reason(delta.stop_reason.as_deref()));
                        }
                        SseEvent::MessageStop => {
                            let stop = stop_reason.take().unwrap_or(StopReason::EndTurn);
                            let _ = tx.send(StreamEvent::Done { stop_reason: stop, usage: usage.clone() }).await;
                            return;
                        }
                        SseEvent::Error { error } => {
                            let msg = error["message"].as_str().unwrap_or("unknown error").to_string();
                            let _ = tx.send(StreamEvent::Error(msg)).await;
                            return;
                        }
                        SseEvent::Ping => {}
                    }
                }
            }
        }
    }

    let stop = stop_reason.take().unwrap_or(StopReason::EndTurn);
    let _ = tx.send(StreamEvent::Done { stop_reason: stop, usage }).await;
}

// ── 模型列表（运行时从 /v1/models 拉取）──────────────────────────────────────

/// `GET /v1/models` 响应：`{ "data": [ { type, id, display_name, created_at } ], ... }`。
/// 该端点只返回标识信息，不含上下文窗口/能力位——能力由 `infer_caps` 按 id 推断。
#[derive(Debug, Deserialize)]
struct ModelsListResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id:           String,
    #[serde(default)]
    display_name: Option<String>,
}

/// 按模型 id 推断能力（列表端点不返回这些字段）。
/// 默认值偏保守，确保 catalog 可用；精确值如需可逐个 `GET /v1/models/{id}` 获取。
fn infer_caps(id: &str) -> ModelInfo {
    let lid = id.to_lowercase();
    // 上下文窗口：Claude 4 系列普遍 200k 起（部分 1M），保守取默认 200k。
    let context_window = DEFAULT_CONTEXT_WINDOW;
    let (max_output_tokens, supports_thinking) = if lid.contains("opus") {
        (32_000, true)
    } else if lid.contains("sonnet") {
        (64_000, true)
    } else if lid.contains("haiku") {
        (8_000, false)
    } else {
        (8_000, false)
    };
    ModelInfo {
        id:               id.to_string(),
        display_name:     id.to_string(), // 调用方可用 ModelEntry.display_name 覆盖
        context_window,
        max_output_tokens,
        supports_vision:  true,
        supports_thinking,
        supports_tools:   true,
    }
}

/// 离线兜底：拉取失败时至少提供默认模型，保证 catalog 非空（Tips §1.3）。
fn fallback_model_info() -> ModelInfo {
    infer_caps(DEFAULT_MODEL)
}
