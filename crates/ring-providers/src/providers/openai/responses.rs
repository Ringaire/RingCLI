//! OpenAI Responses API provider（`/v1/responses`）。
//!
//! Responses API 是 Chat Completions 的演进版，提供：
//! - 语义化流式事件（`response.output_text.delta` 等）
//! - 原生推理（reasoning）输出
//! - 有状态对话（`previous_response_id`）
//! - 更好的缓存利用率
//!
//! 参考：<https://platform.openai.com/docs/guides/migrate-to-responses>

use async_trait::async_trait;
use ring_core::tools::{ContentBlock, Message, MessageRole};
use reqwest::Client;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::error::ProviderError;
use crate::provider::{
    check_response_error, ChatRequest, ChatResponse, ModelInfo, Provider, ProviderStream,
    StopReason, StreamChunk, StreamEvent, ToolDef, Usage,
};

use super::openai_known_models;

const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_MODEL: &str = "gpt-4o";

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> crate::catalog::CatalogEntry {
    crate::catalog::CatalogEntry {
        api_key: None,
        name: "OpenAI Responses".into(),
        kind: crate::catalog::ProviderKind::OpenAiResponses,
        base_url: Some("https://api.openai.com/v1".into()),
        api_key_env: Some("OPENAI_API_KEY".into()),
        default_model: Some("gpt-4o".into()),
        extra_body: None,
    }
}

// ── 消息转换 ──────────────────────────────────────────────────────────────────

/// 把内部消息列表转为 Responses API `input` 数组。
///
/// - User     → `{"role":"user","content":"..."}`
/// - Assistant 纯文本 → `{"role":"assistant","content":[{"type":"output_text","text":"..."}]}`
/// - Assistant 带工具调用 → 拆分为 message item + function_call items
/// - ToolResult → `{"type":"function_call_output","call_id":"...","output":"..."}`
fn convert_messages_to_input(msgs: &[Message]) -> Value {
    let mut out = Vec::new();
    for msg in msgs {
        match msg.role {
            MessageRole::User => {
                // 收集图片（多模态，Responses API 同样用 image_url 格式）
                let images: Vec<(String, String)> = msg.content.iter()
                    .filter_map(|b| if let ContentBlock::Image { media_type, data } = b {
                        Some((media_type.clone(), data.clone()))
                    } else { None })
                    .collect();
                let text: String = msg.content.iter()
                    .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>()
                    .join("\n");
                if images.is_empty() {
                    out.push(json!({"role": "user", "content": text}));
                } else {
                    let mut parts: Vec<Value> = Vec::new();
                    if !text.is_empty() {
                        parts.push(json!({"type": "input_text", "text": text}));
                    }
                    for (media_type, data) in images {
                        parts.push(json!({
                            "type": "input_image",
                            "image_url": format!("data:{};base64,{}", media_type, data)
                        }));
                    }
                    out.push(json!({"role": "user", "content": parts}));
                }
            }
            MessageRole::Assistant => {
                let mut text_parts: Vec<&str> = Vec::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                for blk in &msg.content {
                    match blk {
                        ContentBlock::Text { text } => text_parts.push(text.as_str()),
                        ContentBlock::ToolUse { tool_use_id, tool_name, tool_input } => {
                            tool_calls.push(json!({
                                "type": "function_call",
                                "call_id": tool_use_id,
                                "name": tool_name,
                                "arguments": tool_input.to_string(),
                            }));
                        }
                        ContentBlock::ToolResult { .. } | ContentBlock::Image { .. } | ContentBlock::Thinking { .. } => {}
                    }
                }
                // assistant message item（如果有文本）
                if !text_parts.is_empty() {
                    out.push(json!({
                        "role": "assistant",
                        "content": text_parts.join("\n"),
                    }));
                }
                // function_call items（如果有工具调用）
                for tc in tool_calls {
                    out.push(tc);
                }
            }
            MessageRole::ToolResult => {
                for blk in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, tool_result, .. } = blk {
                        let output_str = tool_result.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| tool_result.to_string());
                        out.push(json!({
                            "type": "function_call_output",
                            "call_id": tool_use_id,
                            "output": output_str,
                        }));
                    }
                }
            }
        }
    }
    Value::Array(out)
}

/// 把 ToolDef 列表转为 Responses API tools 格式。
///
/// Responses API 的工具定义是扁平的（不像 Chat Completions 那样嵌套 `function` 字段）：
/// ```json
/// {"type":"function","name":"...","description":"...","parameters":{...}}
/// ```
fn convert_tools(tools: &[ToolDef]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema,
            }))
            .collect(),
    )
}

// ── Provider 结构 ─────────────────────────────────────────────────────────────

pub struct OpenAiResponsesProvider {
    client:     Client,
    api_key:    String,
    base_url:   String,
    org_id:     Option<String>,
    def_model:  String,
    extra_body: Option<Value>,
}

impl OpenAiResponsesProvider {
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

    pub fn with_extra_body(mut self, extra: Value) -> Self {
        self.extra_body = Some(extra);
        self
    }

    fn build_body(&self, req: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model":  req.model,
            "input":  convert_messages_to_input(&req.messages),
            "stream": stream,
        });

        // system prompt → instructions
        if let Some(system) = &req.system {
            body["instructions"] = json!(system);
        }

        if !req.tools.is_empty() {
            body["tools"] = convert_tools(&req.tools);
        }
        if let Some(t) = req.temperature {
            body["temperature"] = json!((t as f64 * 100.0).round() / 100.0);
        }
        if req.max_tokens > 0 {
            body["max_output_tokens"] = json!(req.max_tokens);
        }
        if let Some(effort) = &req.reasoning_effort {
            body["reasoning"] = json!({"effort": effort});
        }

        // 合并 extra_body
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

    fn add_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut b = builder.bearer_auth(&self.api_key);
        if let Some(org) = &self.org_id {
            b = b.header("OpenAI-Organization", org);
        }
        b
    }
}

// ── Provider trait 实现 ───────────────────────────────────────────────────────

#[async_trait]
impl Provider for OpenAiResponsesProvider {
    fn id(&self) -> &str { "openai-responses" }
    fn display_name(&self) -> &str { "OpenAI Responses" }
    fn default_model(&self) -> &str { &self.def_model }

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError> {
        let url  = format!("{}/responses", self.base_url);
        let body = self.build_body(req, false);
        debug!(model = %req.model, "openai responses chat request");

        let builder = self.add_headers(self.client.post(&url).json(&body));

        let resp = tokio::select! {
            r = builder.send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;
        let raw: Value = resp.json().await.map_err(ProviderError::Network)?;

        parse_responses_output(&raw, &req.model)
    }

    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError> {
        let url  = format!("{}/responses", self.base_url);
        let body = self.build_body(req, true);

        let builder = self.add_headers(
            self.client
                .post(&url)
                .header("Accept", "text/event-stream")
                .json(&body),
        );

        let resp = tokio::select! {
            r = builder.send() => r.map_err(ProviderError::Network)?,
            _ = signal.cancelled() => return Err(ProviderError::Cancelled),
        };
        let resp = check_response_error(resp).await?;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let byte_stream = resp.bytes_stream();
        tokio::spawn(run_responses_sse(byte_stream, signal, tx));
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(openai_known_models())
    }
}

// ── 非流式响应解析 ─────────────────────────────────────────────────────────────

/// 解析 Responses API 非流式输出。
///
/// `output` 数组包含多种 item 类型：
/// - `{"type":"message","role":"assistant","content":[{"type":"output_text","text":"..."}]}`
/// - `{"type":"function_call","call_id":"...","name":"...","arguments":"..."}`
/// - `{"type":"reasoning","summary":[...]}`
fn parse_responses_output(raw: &Value, model: &str) -> Result<ChatResponse, ProviderError> {
    let output = raw["output"]
        .as_array()
        .ok_or_else(|| ProviderError::Other("no output array in response".into()))?;

    let mut content = Vec::new();
    let mut stop_reason = StopReason::EndTurn;

    // status 字段判断停止原因
    if let Some(status) = raw["status"].as_str() {
        stop_reason = match status {
            "completed" => StopReason::EndTurn,
            "incomplete" => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };
    }

    for item in output {
        let item_type = item["type"].as_str().unwrap_or("");
        match item_type {
            "message" => {
                if let Some(content_arr) = item["content"].as_array() {
                    for part in content_arr {
                        if part["type"].as_str() == Some("output_text") {
                            if let Some(text) = part["text"].as_str() {
                                if !text.is_empty() {
                                    content.push(ContentBlock::Text { text: text.to_string() });
                                }
                            }
                        }
                    }
                }
            }
            "function_call" => {
                let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                let name    = item["name"].as_str().unwrap_or("").to_string();
                let args    = item["arguments"].as_str().unwrap_or("{}");
                let input: Value = serde_json::from_str(args)
                    .unwrap_or(Value::Object(Default::default()));
                content.push(ContentBlock::ToolUse {
                    tool_use_id: call_id,
                    tool_name: name,
                    tool_input: input,
                });
                stop_reason = StopReason::ToolUse;
            }
            "reasoning" => {
                // 推理摘要（可选处理）
                if let Some(summary) = item["summary"].as_array() {
                    for s in summary {
                        if let Some(text) = s["text"].as_str() {
                            if !text.is_empty() {
                                // 推理摘要作为文本内容的一部分
                                // 不单独推送，避免污染输出
                                let _ = text;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let usage = Usage {
        input_tokens:          raw["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens:         raw["usage"]["output_tokens"].as_u64().unwrap_or(0),
        cache_creation_tokens: raw["usage"]["input_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0),
        cache_read_tokens:     0,
    };

    let message = Message::new(MessageRole::Assistant, content);
    Ok(ChatResponse { message, stop_reason, usage, model: model.to_string() })
}

// ── 流式 SSE 解析 ──────────────────────────────────────────────────────────────

/// Responses API 流式 SSE 解析协程。
///
/// 语义化事件类型（与 Chat Completions 的 `data: {json}` 不同）：
/// - `event: response.created` — 响应对象创建
/// - `event: response.output_text.delta` — 文本增量
/// - `event: response.output_text.done` — 文本完成
/// - `event: response.function_call_arguments.delta` — 工具参数增量
/// - `event: response.function_call_arguments.done` — 工具参数完成
/// - `event: response.output_item.added` — 新输出项（含 function_call 的 name/call_id）
/// - `event: response.output_item.done` — 输出项完成
/// - `event: response.reasoning_summary_text.delta` — 推理摘要增量
/// - `event: response.completed` — 响应完成（含 usage）
/// - `event: error` — 错误
async fn run_responses_sse<S>(
    byte_stream: S,
    signal:      CancellationToken,
    tx:          tokio::sync::mpsc::Sender<StreamEvent>,
) where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    tokio::pin!(byte_stream);
    let mut buf = String::new();

    // 按 output_index 累积 function_call 信息
    let mut fc_acc: std::collections::HashMap<usize, FunctionCallAcc> =
        std::collections::HashMap::new();
    let mut input_tokens = 0u64;
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
                    Some(Err(e)) => {
                        let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                        return;
                    }
                    Some(Ok(b)) => b,
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Responses API SSE 按 "\n\n" 分割事件块
                while let Some(end) = buf.find("\n\n") {
                    let block = buf[..end].to_string();
                    buf = buf[end + 2..].to_string();

                    // 解析 event: 和 data: 行
                    let mut event_type = String::new();
                    let mut data_str = String::new();
                    for line in block.lines() {
                        if let Some(et) = line.strip_prefix("event: ") {
                            event_type = et.trim().to_string();
                        } else if let Some(d) = line.strip_prefix("data: ") {
                            data_str = d.to_string();
                        }
                    }

                    if event_type.is_empty() && data_str.is_empty() {
                        continue;
                    }

                    // 解析 data JSON
                    let data: Value = if data_str.is_empty() {
                        Value::Null
                    } else {
                        match serde_json::from_str(&data_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        }
                    };

                    match event_type.as_str() {
                        // ── 文本增量 ──
                        "response.output_text.delta" => {
                            if let Some(delta) = data["delta"].as_str() {
                                if !delta.is_empty() {
                                    let _ = tx.send(StreamEvent::Chunk(
                                        StreamChunk::TextDelta { delta: delta.to_string() },
                                    )).await;
                                }
                            }
                        }

                        // ── 文本完成 ──
                        "response.output_text.done" => {
                            if let Some(full) = data["text"].as_str() {
                                if !full.is_empty() {
                                    let _ = tx.send(StreamEvent::Chunk(
                                        StreamChunk::TextDone { full: full.to_string() },
                                    )).await;
                                }
                            }
                        }

                        // ── 推理摘要增量 ──
                        "response.reasoning_summary_text.delta" => {
                            if let Some(delta) = data["delta"].as_str() {
                                if !delta.is_empty() {
                                    let _ = tx.send(StreamEvent::Chunk(
                                        StreamChunk::ThinkingDelta { delta: delta.to_string() },
                                    )).await;
                                }
                            }
                        }

                        // ── 推理摘要完成 ──
                        "response.reasoning_summary_text.done" => {
                            if let Some(full) = data["text"].as_str() {
                                if !full.is_empty() {
                                    let _ = tx.send(StreamEvent::Chunk(
                                        StreamChunk::ThinkingDone { full: full.to_string() },
                                    )).await;
                                }
                            }
                        }

                        // ── 新输出项添加（function_call 的 name/call_id 在此处获得）──
                        "response.output_item.added" => {
                            let output_index = data["output_index"].as_u64().unwrap_or(0) as usize;
                            if let Some(item) = data.get("item") {
                                if item["type"].as_str() == Some("function_call") {
                                    let acc = fc_acc.entry(output_index)
                                        .or_default();
                                    acc.call_id = item["call_id"].as_str()
                                        .map(String::from);
                                    acc.name = item["name"].as_str()
                                        .map(String::from);
                                    // 如果有 name 和 call_id，发送 ToolCallStart
                                    if let (Some(id), Some(name)) = (&acc.call_id, &acc.name) {
                                        if !id.is_empty() && !name.is_empty() {
                                            acc.started = true;
                                            let _ = tx.send(StreamEvent::Chunk(
                                                StreamChunk::ToolCallStart {
                                                    call_id: id.clone(),
                                                    tool_name: name.clone(),
                                                },
                                            )).await;
                                        }
                                    }
                                }
                            }
                        }

                        // ── function_call 参数增量 ──
                        "response.function_call_arguments.delta" => {
                            let output_index = data["output_index"].as_u64().unwrap_or(0) as usize;
                            if let Some(delta) = data["delta"].as_str() {
                                let acc = fc_acc.entry(output_index)
                                    .or_default();
                                acc.args.push_str(delta);
                                if let Some(id) = &acc.call_id {
                                    let _ = tx.send(StreamEvent::Chunk(
                                        StreamChunk::ToolCallInput {
                                            call_id: id.clone(),
                                            delta: delta.to_string(),
                                        },
                                    )).await;
                                }
                            }
                        }

                        // ── function_call 参数完成 ──
                        "response.function_call_arguments.done" => {
                            let output_index = data["output_index"].as_u64().unwrap_or(0) as usize;
                            if let Some(full_args) = data["arguments"].as_str() {
                                let acc = fc_acc.entry(output_index)
                                    .or_default();
                                acc.args = full_args.to_string();
                            }
                        }

                        // ── 输出项完成 ──
                        "response.output_item.done" => {
                            let output_index = data["output_index"].as_u64().unwrap_or(0) as usize;
                            if let Some(item) = data.get("item") {
                                if item["type"].as_str() == Some("function_call") {
                                    // 确保 call_id 和 name 都有
                                    let acc = fc_acc.entry(output_index)
                                        .or_default();
                                    if acc.call_id.is_none() {
                                        acc.call_id = item["call_id"].as_str().map(String::from);
                                    }
                                    if acc.name.is_none() {
                                        acc.name = item["name"].as_str().map(String::from);
                                    }
                                    if let Some(args) = item["arguments"].as_str() {
                                        acc.args = args.to_string();
                                    }
                                }
                            }
                        }

                        // ── 响应完成 ──
                        "response.completed" => {
                            // 提取 usage
                            if let Some(u) = data.get("response")
                                .and_then(|r| r.get("usage"))
                            {
                                input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                                output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
                            }

                            // 从 response.output 检查是否有 function_call
                            if let Some(outputs) = data.get("response")
                                .and_then(|r| r.get("output"))
                                .and_then(|o| o.as_array())
                            {
                                let has_fc = outputs.iter()
                                    .any(|item| item["type"].as_str() == Some("function_call"));
                                if has_fc {
                                    final_stop = StopReason::ToolUse;
                                }
                            }

                            // flush 所有累积的 function_call
                            flush_function_calls(&fc_acc, &tx).await;

                            let _ = tx.send(StreamEvent::Done {
                                stop_reason: final_stop.clone(),
                                usage: Usage {
                                    input_tokens,
                                    output_tokens,
                                    cache_creation_tokens: 0,
                                    cache_read_tokens: 0,
                                },
                            }).await;
                            return;
                        }

                        // ── 错误 ──
                        "error" => {
                            let msg = data["message"].as_str()
                                .or_else(|| data["error"].as_str())
                                .unwrap_or("unknown error");
                            let _ = tx.send(StreamEvent::Error(msg.to_string())).await;
                            return;
                        }

                        // ── 响应失败 ──
                        "response.failed" => {
                            let msg = data["response"]["error"]["message"].as_str()
                                .unwrap_or("response failed");
                            let _ = tx.send(StreamEvent::Error(msg.to_string())).await;
                            return;
                        }

                        _ => {
                            // 忽略未处理的事件类型
                        }
                    }
                }
            }
        }
    }

    // 流结束但未收到 response.completed（容错）
    flush_function_calls(&fc_acc, &tx).await;
    let _ = tx.send(StreamEvent::Done {
        stop_reason: final_stop,
        usage: Usage {
            input_tokens,
            output_tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        },
    }).await;
}

/// function_call 累积器
#[derive(Default)]
struct FunctionCallAcc {
    call_id: Option<String>,
    name:    Option<String>,
    args:    String,
    started: bool,
}

/// flush 所有累积的 function_call 为 ToolCallDone 事件。
async fn flush_function_calls(
    fc_acc: &std::collections::HashMap<usize, FunctionCallAcc>,
    tx:     &tokio::sync::mpsc::Sender<StreamEvent>,
) {
    let mut sorted: Vec<_> = fc_acc.iter().collect();
    sorted.sort_by_key(|(k, _)| *k);
    for (_, acc) in sorted {
        if let Some(id) = &acc.call_id {
            let full_input: Value = serde_json::from_str(&acc.args)
                .unwrap_or(Value::Object(Default::default()));
            let _ = tx.send(StreamEvent::Chunk(StreamChunk::ToolCallDone {
                call_id: id.clone(),
                full_input,
            })).await;
        }
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_messages_to_input_user() {
        let msgs = vec![Message::user_text("Hello")];
        let input = convert_messages_to_input(&msgs);
        let arr = input.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["role"], "user");
        assert_eq!(arr[0]["content"], "Hello");
    }

    #[test]
    fn test_convert_messages_to_input_with_tool_result() {
        let msgs = vec![Message::new(MessageRole::ToolResult, vec![
            ContentBlock::ToolResult {
                tool_use_id: "call_123".into(),
                tool_result: json!({"temp": 72}),
                is_error: false,
            },
        ])];
        let input = convert_messages_to_input(&msgs);
        let arr = input.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "function_call_output");
        assert_eq!(arr[0]["call_id"], "call_123");
        assert_eq!(arr[0]["output"], json!({"temp": 72}).to_string());
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![ToolDef {
            name: "get_weather".into(),
            description: "Get weather".into(),
            input_schema: json!({"type":"object","properties":{"location":{"type":"string"}}}),
        }];
        let result = convert_tools(&tools);
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["type"], "function");
        assert_eq!(arr[0]["name"], "get_weather");
        assert_eq!(arr[0]["description"], "Get weather");
        assert!(arr[0].get("function").is_none(), "Responses API tools should not have nested 'function' field");
    }

    #[test]
    fn test_parse_responses_output_text() {
        let raw = json!({
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "Hello world"}
                    ]
                }
            ],
            "status": "completed",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });
        let resp = parse_responses_output(&raw, "gpt-4o").unwrap();
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert_eq!(resp.message.content.len(), 1);
        match &resp.message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn test_parse_responses_output_function_call() {
        let raw = json!({
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Let me check."}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "get_weather",
                    "arguments": "{\"location\":\"SF\"}"
                }
            ],
            "status": "completed",
            "usage": {"input_tokens": 15, "output_tokens": 10}
        });
        let resp = parse_responses_output(&raw, "gpt-4o").unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.message.content.len(), 2);
        match &resp.message.content[1] {
            ContentBlock::ToolUse { tool_use_id, tool_name, tool_input } => {
                assert_eq!(tool_use_id, "call_abc");
                assert_eq!(tool_name, "get_weather");
                assert_eq!(tool_input["location"], "SF");
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn test_catalog_entry() {
        let entry = catalog_entry();
        assert_eq!(entry.kind, crate::catalog::ProviderKind::OpenAiResponses);
        assert_eq!(entry.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
    }

    #[test]
    fn test_build_body_basic() {
        let provider = OpenAiResponsesProvider::new("test-key", None, None, None);
        let req = ChatRequest::new("gpt-4o", vec![Message::user_text("Hi")]);
        let body = provider.build_body(&req, false);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["stream"], false);
        assert!(body.get("input").is_some());
        assert!(body.get("messages").is_none(), "should use 'input' not 'messages'");
    }

    #[test]
    fn test_build_body_with_instructions() {
        let provider = OpenAiResponsesProvider::new("test-key", None, None, None);
        let mut req = ChatRequest::new("gpt-4o", vec![Message::user_text("Hi")]);
        req.system = Some("You are helpful.".into());
        let body = provider.build_body(&req, false);
        assert_eq!(body["instructions"], "You are helpful.");
    }

    #[test]
    fn test_build_body_with_tools() {
        let provider = OpenAiResponsesProvider::new("test-key", None, None, None);
        let mut req = ChatRequest::new("gpt-4o", vec![Message::user_text("Hi")]);
        req.tools = vec![ToolDef {
            name: "search".into(),
            description: "Search".into(),
            input_schema: json!({"type":"object"}),
        }];
        let body = provider.build_body(&req, false);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "search");
        assert!(tools[0].get("function").is_none());
    }

    #[test]
    fn test_build_body_with_reasoning_effort() {
        let provider = OpenAiResponsesProvider::new("test-key", None, None, None);
        let mut req = ChatRequest::new("o3-mini", vec![Message::user_text("Think")]);
        req.reasoning_effort = Some("high".into());
        let body = provider.build_body(&req, false);
        assert_eq!(body["reasoning"]["effort"], "high");
    }

    #[test]
    fn test_build_body_max_tokens() {
        let provider = OpenAiResponsesProvider::new("test-key", None, None, None);
        let mut req = ChatRequest::new("gpt-4o", vec![Message::user_text("Hi")]);
        req.max_tokens = 4096;
        let body = provider.build_body(&req, false);
        assert_eq!(body["max_output_tokens"], 4096);
        assert!(body.get("max_tokens").is_none(), "should use 'max_output_tokens' not 'max_tokens'");
    }
}
