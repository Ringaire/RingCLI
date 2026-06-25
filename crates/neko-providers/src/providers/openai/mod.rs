pub mod oauth;

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> crate::catalog::CatalogEntry {
    crate::catalog::CatalogEntry {
        name: "OpenAI".into(),
        kind: crate::catalog::ProviderKind::OpenAi,
        base_url: Some("https://api.openai.com/v1".into()),
        api_key_env: Some("OPENAI_API_KEY".into()),
        default_model: Some("gpt-4o".into()),
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

const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_MODEL: &str = "gpt-4o";

/// 把内部消息列表转为 OpenAI Chat API 格式。
///
/// - User     → `{"role":"user","content":"..."}`
/// - Assistant 纯文本 → `{"role":"assistant","content":"..."}`
/// - Assistant 带工具调用 → `{"role":"assistant","content":null,"tool_calls":[...]}`
/// - ToolResult → 每个 ToolResult block 单独一条 `{"role":"tool","tool_call_id":"...","content":"..."}`
fn convert_messages(msgs: &[Message]) -> Value {
    let mut out = Vec::new();
    for msg in msgs {
        match msg.role {
            MessageRole::User => {
                let text: String = msg.content.iter()
                    .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>()
                    .join("\n");
                out.push(json!({"role": "user", "content": text}));
            }
            MessageRole::Assistant => {
                let mut text_parts: Vec<&str> = Vec::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                for blk in &msg.content {
                    match blk {
                        ContentBlock::Text { text } => text_parts.push(text.as_str()),
                        ContentBlock::ToolUse { tool_use_id, tool_name, tool_input } => {
                            tool_calls.push(json!({
                                "id":   tool_use_id,
                                "type": "function",
                                "function": {
                                    "name":      tool_name,
                                    "arguments": tool_input.to_string(),
                                }
                            }));
                        }
                        ContentBlock::ToolResult { .. } => {}
                    }
                }
                let content_val = if text_parts.is_empty() {
                    Value::Null
                } else {
                    json!(text_parts.join("\n"))
                };
                let mut entry = json!({"role": "assistant", "content": content_val});
                if !tool_calls.is_empty() {
                    entry["tool_calls"] = json!(tool_calls);
                }
                out.push(entry);
            }
            MessageRole::ToolResult => {
                // 每个 ToolResult block → 独立的 role:tool 消息
                for blk in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, tool_result, .. } = blk {
                        // Value::String 用 as_str() 避免额外引号；其他类型序列化为 JSON 字符串
                        let content_str = tool_result.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| tool_result.to_string());
                        out.push(json!({
                            "role":         "tool",
                            "tool_call_id": tool_use_id,
                            "content":      content_str,
                        }));
                    }
                }
            }
        }
    }
    Value::Array(out)
}

fn convert_tools(tools: &[ToolDef]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            }))
            .collect(),
    )
}

fn parse_finish_reason(s: Option<&str>) -> StopReason {
    match s {
        Some("stop")         => StopReason::EndTurn,
        Some("tool_calls")   => StopReason::ToolUse,
        Some("length")       => StopReason::MaxTokens,
        _                    => StopReason::EndTurn,
    }
}

pub struct OpenAiProvider {
    client:     Client,
    api_key:    String,
    base_url:   String,
    org_id:     Option<String>,
    def_model:  String,
    extra_body: Option<serde_json::Value>,
}

impl OpenAiProvider {
    pub fn new(
        api_key:   impl Into<String>,
        base_url:  Option<String>,
        org_id:    Option<String>,
        def_model: Option<String>,
    ) -> Self {
        let client = crate::provider::build_http_client(None, CONNECT_TIMEOUT_SECS);
        Self::with_client(client, api_key, base_url, org_id, def_model)
    }

    pub fn with_client(
        client:    Client,
        api_key:   impl Into<String>,
        base_url:  Option<String>,
        org_id:    Option<String>,
        def_model: Option<String>,
    ) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.unwrap_or_else(|| OPENAI_BASE_URL.to_string()),
            org_id,
            def_model: def_model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            extra_body: None,
        }
    }

    pub fn with_extra_body(mut self, extra: serde_json::Value) -> Self {
        self.extra_body = Some(extra);
        self
    }

    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model":    req.model,
            "messages": convert_messages(&req.messages),
            "stream":   stream,
        });
        if !req.tools.is_empty() {
            body["tools"] = convert_tools(&req.tools);
        }
        if let Some(t) = req.temperature {
            // round 到 2 位小数，避免 f32 精度溢出（某些 provider 限制小数位数）
            body["temperature"] = json!((t as f64 * 100.0).round() / 100.0);
        }
        if req.max_tokens > 0 {
            body["max_tokens"] = json!(req.max_tokens);
        }
        // reasoning effort（OpenAI o-series / 智谱 GLM-5+）
        if let Some(effort) = &req.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }
        // 合并 extra_body（catalog 注入的 provider 特有字段，如 Ollama 的 options.num_ctx）
        if let Some(extra) = &self.extra_body {
            if let Some(obj) = extra.as_object() {
                let body_obj = body.as_object_mut().unwrap();
                for (k, v) in obj {
                    body_obj.entry(k).or_insert_with(|| v.clone());
                }
            }
        }
        body
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &str { "openai" }
    fn display_name(&self) -> &str { "OpenAI" }
    fn default_model(&self) -> &str { &self.def_model }

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError> {
        let url  = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(req, false);
        debug!(model = %req.model, "openai chat request");

        let mut builder = self.client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body);
        if let Some(org) = &self.org_id {
            builder = builder.header("OpenAI-Organization", org);
        }

        let resp = tokio::select! {
            r = builder.send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;
        let raw: Value = resp.json().await.map_err(ProviderError::Network)?;

        let choice = raw["choices"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| ProviderError::Other("no choices in response".into()))?;

        let finish_reason = choice["finish_reason"].as_str();
        let stop_reason   = parse_finish_reason(finish_reason);

        let usage = Usage {
            input_tokens:          raw["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens:         raw["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            cache_creation_tokens: 0,
            cache_read_tokens:     0,
        };

        let msg_val  = &choice["message"];
        let role     = MessageRole::Assistant;
        let mut content = Vec::new();

        if let Some(text) = msg_val["content"].as_str() {
            if !text.is_empty() {
                content.push(ContentBlock::Text { text: text.to_string() });
            }
        }
        if let Some(tc) = msg_val["tool_calls"].as_array() {
            for call in tc {
                let id   = call["id"].as_str().unwrap_or("").to_string();
                let name = call["function"]["name"].as_str().unwrap_or("").to_string();
                let args = call["function"]["arguments"].as_str().unwrap_or("{}");
                let input: Value = serde_json::from_str(args).unwrap_or(Value::Object(Default::default()));
                content.push(ContentBlock::ToolUse { tool_use_id: id, tool_name: name, tool_input: input });
            }
        }

        let message = Message::new(role, content);
        Ok(ChatResponse { message, stop_reason, usage, model: req.model.clone() })
    }

    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError> {
        let url  = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(req, true);

        let mut builder = self.client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Accept", "text/event-stream")
            .json(&body);
        if let Some(org) = &self.org_id {
            builder = builder.header("OpenAI-Organization", org);
        }

        let resp = tokio::select! {
            r = builder.send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let byte_stream = resp.bytes_stream();
        tokio::spawn(run_openai_sse(byte_stream, signal, tx));
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(openai_known_models())
    }
}

async fn run_openai_sse<S>(
    byte_stream: S,
    signal:      CancellationToken,
    tx:          tokio::sync::mpsc::Sender<StreamEvent>,
) where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    tokio::pin!(byte_stream);
    let mut buf = String::new();

    // 按 index 累积工具调用：(call_id, tool_name, args_so_far, started)
    let mut tool_acc: std::collections::HashMap<usize, (String, String, String, bool)> =
        std::collections::HashMap::new();
    let mut input_tokens  = 0u64;
    let mut output_tokens = 0u64;
    let mut final_stop = StopReason::EndTurn;

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
                    if data == "[DONE]" {
                        flush_tool_calls(&tool_acc, &tx).await;
                        let _ = tx.send(StreamEvent::Done {
                            stop_reason: final_stop.clone(),
                            usage: Usage { input_tokens, output_tokens, ..Usage::default() },
                        }).await;
                        return;
                    }
                    let raw: Value = match serde_json::from_str(&data) { Ok(v) => v, Err(_) => continue };

                    // usage 字段（部分 provider 在中间 chunk 里带）
                    if let Some(u) = raw.get("usage") {
                        input_tokens  = u["prompt_tokens"].as_u64().unwrap_or(input_tokens);
                        output_tokens = u["completion_tokens"].as_u64().unwrap_or(output_tokens);
                    }

                    let choices = match raw["choices"].as_array() {
                        Some(a) if !a.is_empty() => a.clone(),
                        _ => continue,
                    };
                    let choice = &choices[0];
                    let delta  = &choice["delta"];

                    // ── 推理 delta（DeepSeek 用 `reasoning_content`，OpenRouter 等用 `reasoning`）──
                    if let Some(r) = delta["reasoning_content"].as_str()
                        .or_else(|| delta["reasoning"].as_str())
                    {
                        if !r.is_empty() {
                            let _ = tx.send(StreamEvent::Chunk(StreamChunk::ThinkingDelta {
                                delta: r.to_string(),
                            })).await;
                        }
                    }

                    // ── 文本 delta ──
                    if let Some(text) = delta["content"].as_str() {
                        if !text.is_empty() {
                            let _ = tx.send(StreamEvent::Chunk(StreamChunk::TextDelta {
                                delta: text.to_string(),
                            })).await;
                        }
                    }

                    // ── 工具调用 delta ──
                    if let Some(tc_arr) = delta["tool_calls"].as_array() {
                        for tc in tc_arr {
                            let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                            let entry = tool_acc.entry(idx)
                                .or_insert_with(|| (String::new(), String::new(), String::new(), false));

                            if let Some(id) = tc["id"].as_str() {
                                entry.0 = id.to_string();
                            }
                            if let Some(name) = tc["function"]["name"].as_str() {
                                if !name.is_empty() {
                                    entry.1 = name.to_string();
                                }
                            }
                            // 首次有 id + name 时发 ToolCallStart
                            if !entry.3 && !entry.0.is_empty() && !entry.1.is_empty() {
                                entry.3 = true;
                                let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallStart {
                                    call_id:   entry.0.clone(),
                                    tool_name: entry.1.clone(),
                                })).await;
                            }
                            if let Some(args_delta) = tc["function"]["arguments"].as_str() {
                                if !args_delta.is_empty() {
                                    entry.2.push_str(args_delta);
                                    let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallInput {
                                        call_id: entry.0.clone(),
                                        delta:   args_delta.to_string(),
                                    })).await;
                                }
                            }
                        }
                    }

                    // ── finish_reason ──
                    if let Some(finish) = choice["finish_reason"].as_str() {
                        if !finish.is_empty() {
                            final_stop = parse_finish_reason(Some(finish));
                            flush_tool_calls(&tool_acc, &tx).await;
                            let _ = tx.send(StreamEvent::Done {
                                stop_reason: final_stop.clone(),
                                usage: Usage { input_tokens, output_tokens, ..Usage::default() },
                            }).await;
                            return;
                        }
                    }
                }
            }
        }
    }
    flush_tool_calls(&tool_acc, &tx).await;
    let _ = tx.send(StreamEvent::Done {
        stop_reason: final_stop,
        usage: Usage { input_tokens, output_tokens, ..Usage::default() },
    }).await;
}

/// 把累积完的工具调用统一发出 ToolCallDone 事件。
async fn flush_tool_calls(
    tool_acc: &std::collections::HashMap<usize, (String, String, String, bool)>,
    tx:       &tokio::sync::mpsc::Sender<StreamEvent>,
) {
    let mut sorted: Vec<_> = tool_acc.iter().collect();
    sorted.sort_by_key(|(k, _)| *k);
    for (_, (id, _name, args, _)) in sorted {
        let full_input: Value = serde_json::from_str(args)
            .unwrap_or(Value::Object(Default::default()));
        let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallDone {
            call_id:    id.clone(),
            full_input,
        })).await;
    }
}

/// OpenAI 主流模型的统一上下文窗口（gpt-4o / o 系列均 128k）。
const GPT_CONTEXT_WINDOW: u64 = 128_000;

fn openai_known_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gpt-4o".into(),
            display_name: "GPT-4o".into(),
            context_window: GPT_CONTEXT_WINDOW,
            max_output_tokens: 16_000,
            supports_vision: true,
            supports_thinking: false,
            supports_tools: true,
        },
        ModelInfo {
            id: "gpt-4o-mini".into(),
            display_name: "GPT-4o mini".into(),
            context_window: GPT_CONTEXT_WINDOW,
            max_output_tokens: 16_000,
            supports_vision: true,
            supports_thinking: false,
            supports_tools: true,
        },
        ModelInfo {
            id: "o1".into(),
            display_name: "o1".into(),
            context_window: GPT_CONTEXT_WINDOW,
            max_output_tokens: 32_000,
            supports_vision: true,
            supports_thinking: true,
            supports_tools: true,
        },
        ModelInfo {
            id: "o3-mini".into(),
            display_name: "o3-mini".into(),
            context_window: GPT_CONTEXT_WINDOW,
            max_output_tokens: 65_536,
            supports_vision: false,
            supports_thinking: true,
            supports_tools: true,
        },
    ]
}
