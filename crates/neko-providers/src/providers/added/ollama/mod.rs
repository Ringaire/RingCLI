// Ollama 原生 /api/chat provider
//
// Ollama 的 OpenAI-compatible 端点（/v1/chat/completions）不支持 thinking 参数，
// 因此使用原生 /api/chat 端点，完整支持 think、num_predict、tools 等。
//

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目（本地，无需 API key）。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Ollama".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("http://localhost:11434/v1".into()),
        api_key_env: None,
        default_model: Some("llama3.2".into()),
        extra_body: Some(serde_json::json!({"options": {"num_ctx": 32768}})),
    }
}
// 流式响应使用 NDJSON（每行一个 JSON 对象），不是 SSE。

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

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const CONNECT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MODEL: &str = "llama3.2";

// ── 消息转换 ──────────────────────────────────────────────────────────────────

/// 把内部消息列表转为 Ollama 原生格式。
///
/// - User     → `{"role":"user","content":"..."}`
/// - Assistant 纯文本 → `{"role":"assistant","content":"..."}`
/// - Assistant 带工具调用 → `{"role":"assistant","content":null,"tool_calls":[...]}`
/// - ToolResult → `{"role":"tool","content":"..."}`
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
                for blk in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, tool_result, .. } = blk {
                        let content_str = tool_result.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| tool_result.to_string());
                        out.push(json!({
                            "role":    "tool",
                            "content": content_str,
                            "tool_call_id": tool_use_id,
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
        tools.iter().map(|t| json!({
            "type": "function",
            "function": {
                "name":        t.name,
                "description": t.description,
                "parameters":  t.input_schema,
            }
        })).collect(),
    )
}

// ── Provider 实现 ─────────────────────────────────────────────────────────────

pub struct OllamaProvider {
    client:     Client,
    base_url:   String,
    def_model:  String,
    num_ctx:    u32,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>, def_model: Option<String>, num_ctx: Option<u32>) -> Self {
        let client = crate::provider::build_http_client(None, CONNECT_TIMEOUT_SECS);
        Self::with_client(client, base_url, def_model, num_ctx)
    }

    pub fn with_client(
        client:    Client,
        base_url:  Option<String>,
        def_model: Option<String>,
        num_ctx:   Option<u32>,
    ) -> Self {
        Self {
            client,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            def_model: def_model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            num_ctx: num_ctx.unwrap_or(32768),
        }
    }

    /// 构建 Ollama 原生 /api/chat 请求体。
    ///
    /// 核心修复：将 `extended_thinking` → `think` 参数传入 Ollama，
    /// 将 `thinking_budget` → `num_predict` 参数传入 Ollama。
    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model":    req.model,
            "messages": convert_messages(&req.messages),
            "stream":   stream,
        });

        // ── thinking 控制 ──
        // Ollama 的 think 参数控制模型是否启用推理模式。
        // 对于 qwen3 等模型，think=true 会启用深度推理；think=false 会跳过。
        if req.extended_thinking {
            body["think"] = json!(true);
        } else {
            body["think"] = json!(false);
        }

        // ── options ──
        let mut options = json!({
            "num_ctx": self.num_ctx,
        });

        // thinking_budget → num_predict（限制输出 token 数）
        if let Some(budget) = req.thinking_budget {
            options["num_predict"] = json!(budget);
        } else if req.max_tokens > 0 {
            options["num_predict"] = json!(req.max_tokens);
        }

        if let Some(t) = req.temperature {
            options["temperature"] = json!(t);
        }
        if let Some(tp) = req.top_p {
            options["top_p"] = json!(tp);
        }
        if !req.stop.is_empty() {
            options["stop"] = json!(req.stop);
        }

        body["options"] = options;

        // ── tools ──
        if !req.tools.is_empty() {
            body["tools"] = convert_tools(&req.tools);
        }

        body
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> &str { "ollama" }
    fn display_name(&self) -> &str { "Ollama" }
    fn default_model(&self) -> &str { &self.def_model }

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError> {
        let url  = format!("{}/api/chat", self.base_url);
        let body = self.build_body(req, false);
        debug!(model = %req.model, think = %req.extended_thinking, budget = ?req.thinking_budget, "ollama chat request");

        let resp = tokio::select! {
            r = self.client.post(&url).json(&body).send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;
        let raw: Value = resp.json().await.map_err(ProviderError::Network)?;

        let message_val = &raw["message"];
        let role = MessageRole::Assistant;
        let mut content = Vec::new();

        // 文本内容（thinking 内容不存入消息历史，只在当轮显示）
        if let Some(text) = message_val["content"].as_str() {
            if !text.is_empty() {
                content.push(ContentBlock::Text { text: text.to_string() });
            }
        }

        // 工具调用
        if let Some(tc) = message_val["tool_calls"].as_array() {
            for call in tc {
                let id   = call["id"].as_str().unwrap_or("").to_string();
                let name = call["function"]["name"].as_str().unwrap_or("").to_string();
                let args = call["function"]["arguments"].as_str().unwrap_or("{}");
                let input: Value = serde_json::from_str(args).unwrap_or(Value::Object(Default::default()));
                content.push(ContentBlock::ToolUse { tool_use_id: id, tool_name: name, tool_input: input });
            }
        }

        let message = Message::new(role, content);

        let usage = Usage {
            input_tokens:  raw["prompt_eval_count"].as_u64().unwrap_or(0),
            output_tokens: raw["eval_count"].as_u64().unwrap_or(0),
            cache_creation_tokens: 0,
            cache_read_tokens:     0,
        };

        let stop_reason = if raw["done"].as_bool().unwrap_or(false) {
            StopReason::EndTurn
        } else {
            StopReason::EndTurn
        };

        Ok(ChatResponse { message, stop_reason, usage, model: req.model.clone() })
    }

    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError> {
        let url  = format!("{}/api/chat", self.base_url);
        let body = self.build_body(req, true);
        debug!(model = %req.model, think = %req.extended_thinking, "ollama stream request");

        let resp = tokio::select! {
            r = self.client.post(&url).json(&body).send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let byte_stream = resp.bytes_stream();
        tokio::spawn(run_ollama_ndjson(byte_stream, signal, tx));
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        // 调用 /api/tags 获取本地已安装的模型列表
        let url = format!("{}/api/tags", self.base_url);
        let resp = self.client.get(&url).send().await.map_err(ProviderError::Network)?;
        let raw: Value = resp.json().await.map_err(ProviderError::Network)?;

        let mut models = Vec::new();
        if let Some(arr) = raw["models"].as_array() {
            for m in arr {
                let id = m["name"].as_str().unwrap_or("").to_string();
                if id.is_empty() { continue; }
                let size = m["size"].as_u64().unwrap_or(0);
                // 根据模型大小估算上下文窗口
                let ctx = if size > 10_000_000_000 { 32768 } else { 8192 };
                models.push(ModelInfo {
                    id: id.clone(),
                    display_name: id,
                    context_window: ctx,
                    max_output_tokens: 4096,
                    supports_vision: false,
                    supports_thinking: true, // Ollama 模型大多支持 think 模式
                    supports_tools: true,
                });
            }
        }
        Ok(models)
    }
}

// ── 流式解析（NDJSON）────────────────────────────────────────────────────────

/// Ollama 流式响应使用 NDJSON 格式（每行一个 JSON 对象），不是 SSE。
///
/// 每个 chunk 格式：
/// ```json
/// {"model":"qwen3","message":{"role":"assistant","content":"token","thinking":"reason"},"done":false}
/// {"model":"qwen3","message":{"role":"assistant","content":"","thinking":""},"done":true,"done_reason":"stop","total_duration":1234,"prompt_eval_count":100,"eval_count":50}
/// ```
async fn run_ollama_ndjson<S>(
    byte_stream: S,
    signal:      CancellationToken,
    tx:          tokio::sync::mpsc::Sender<StreamEvent>,
) where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    tokio::pin!(byte_stream);
    let mut buf = String::new();
    let mut input_tokens  = 0u64;
    let mut output_tokens = 0u64;

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

                // NDJSON：按行分割
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim().to_string();
                    buf = buf[pos + 1..].to_string();

                    if line.is_empty() { continue; }
                    let raw: Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let message = &raw["message"];

                    // ── thinking delta ──
                    if let Some(thinking) = message["thinking"].as_str() {
                        if !thinking.is_empty() {
                            let _ = tx.send(StreamEvent::Chunk(StreamChunk::ThinkingDelta {
                                delta: thinking.to_string(),
                            })).await;
                        }
                    }

                    // ── text delta ──
                    if let Some(text) = message["content"].as_str() {
                        if !text.is_empty() {
                            let _ = tx.send(StreamEvent::Chunk(StreamChunk::TextDelta {
                                delta: text.to_string(),
                            })).await;
                        }
                    }

                    // ── tool_calls ──
                    if let Some(tc_arr) = message["tool_calls"].as_array() {
                        for tc in tc_arr {
                            let call_id = tc["id"].as_str().unwrap_or("").to_string();
                            let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                            let args = tc["function"]["arguments"].as_str().unwrap_or("{}");
                            
                            if !call_id.is_empty() && !tool_name.is_empty() {
                                let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallStart {
                                    call_id: call_id.clone(),
                                    tool_name: tool_name.clone(),
                                })).await;
                                let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallInput {
                                    call_id: call_id.clone(),
                                    delta: args.to_string(),
                                })).await;
                                let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallDone {
                                    call_id,
                                    full_input: serde_json::from_str(args).unwrap_or(Value::Object(Default::default())),
                                })).await;
                            }
                        }
                    }

                    // ── usage（最后一个 chunk 带 done=true）──
                    if let Some(p) = raw["prompt_eval_count"].as_u64() {
                        input_tokens = p;
                    }
                    if let Some(e) = raw["eval_count"].as_u64() {
                        output_tokens = e;
                    }

                    // ── done ──
                    if raw["done"].as_bool().unwrap_or(false) {
                        let _ = tx.send(StreamEvent::Done {
                            stop_reason: StopReason::EndTurn,
                            usage: Usage { input_tokens, output_tokens, ..Usage::default() },
                        }).await;
                        return;
                    }
                }
            }
        }
    }

    // stream 结束但未收到 done
    let _ = tx.send(StreamEvent::Done {
        stop_reason: StopReason::EndTurn,
        usage: Usage { input_tokens, output_tokens, ..Usage::default() },
    }).await;
}
