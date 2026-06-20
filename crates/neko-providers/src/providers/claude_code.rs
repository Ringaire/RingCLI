// Claude Code provider：通过 `claude -p` 子进程调用，适用于 claude.ai 订阅用户。
// 不需要 API key——复用 `claude auth login` 已缓存的 OAuth 凭证。

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use neko_core::tools::{ContentBlock, Message, MessageRole};

use crate::error::ProviderError;
use crate::provider::{
    ChatRequest, ChatResponse, ModelInfo, Provider, ProviderStream,
    StopReason, StreamChunk, StreamEvent, Usage,
};

// ── stream-json 事件结构 ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CcLine {
    #[serde(rename = "type")]
    kind: String,
    // stream_event
    event: Option<CcEvent>,
    // result
    subtype: Option<String>,
    is_error: Option<bool>,
    result: Option<String>,
    usage: Option<CcUsage>,
    // error message from claude itself
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CcEvent {
    #[serde(rename = "type")]
    kind: String,
    delta: Option<CcDelta>,
    #[serde(rename = "stop_reason")]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CcDelta {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CcUsage {
    input_tokens:                Option<u64>,
    output_tokens:               Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens:     Option<u64>,
}

impl From<CcUsage> for Usage {
    fn from(u: CcUsage) -> Self {
        Self {
            input_tokens:          u.input_tokens.unwrap_or(0),
            output_tokens:         u.output_tokens.unwrap_or(0),
            cache_creation_tokens: u.cache_creation_input_tokens.unwrap_or(0),
            cache_read_tokens:     u.cache_read_input_tokens.unwrap_or(0),
        }
    }
}

// ── ClaudeCodeProvider ───────────────────────────────────────────────────────

pub struct ClaudeCodeProvider {
    claude_bin: PathBuf,
}

impl ClaudeCodeProvider {
    pub fn new(claude_bin: PathBuf) -> Self {
        Self { claude_bin }
    }

    /// 检测 PATH 中是否存在 `claude` 二进制，返回 provider 实例。
    pub fn detect() -> Option<Self> {
        // 先查 PATH
        if let Ok(output) = std::process::Command::new("which")
            .arg("claude")
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(Self::new(PathBuf::from(path)));
                }
            }
        }
        // 常见安装路径兜底
        for p in &[
            "/usr/local/bin/claude",
            "/usr/bin/claude",
        ] {
            if std::path::Path::new(p).exists() {
                return Some(Self::new(PathBuf::from(p)));
            }
        }
        None
    }

    /// 将 ChatRequest 序列化为 CC 的 stream-json 输入行。
    fn build_input_lines(req: &ChatRequest) -> String {
        let mut lines = Vec::new();

        // 系统提示词
        if let Some(sys) = &req.system {
            if !sys.trim().is_empty() {
                lines.push(json!({ "type": "system", "content": sys }).to_string());
            }
        }

        // 消息历史：user/assistant 均用 type:"user"，role 区分在 message.role 里
        for msg in &req.messages {
            let role = match msg.role {
                MessageRole::User | MessageRole::ToolResult => "user",
                MessageRole::Assistant => "assistant",
            };
            let content: Vec<Value> = msg.content.iter().filter_map(|blk| match blk {
                ContentBlock::Text { text } => {
                    Some(json!({ "type": "text", "text": text }))
                }
                // tool_use / tool_result 跳过——CC 不接管 neko 的工具调用
                _ => None,
            }).collect();

            if content.is_empty() { continue; }
            lines.push(json!({
                "type": "user",
                "message": { "role": role, "content": content }
            }).to_string());
        }

        lines.join("\n") + "\n"
    }

    /// 启动 claude 子进程。
    fn spawn_child(&self, model: &str) -> std::io::Result<tokio::process::Child> {
        let mut cmd = Command::new(&self.claude_bin);
        cmd.args([
            "-p",
            "--output-format", "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--input-format",   "stream-json",
        ]);
        if !model.is_empty() {
            cmd.args(["--model", model]);
        }
        cmd.stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::piped())
           .stderr(std::process::Stdio::null())
           .spawn()
    }
}

#[async_trait]
impl Provider for ClaudeCodeProvider {
    fn id(&self)           -> &str { "claude-code" }
    fn display_name(&self) -> &str { "Claude Code (claude.ai)" }
    fn default_model(&self) -> &str { "claude-opus-4-8" }

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError> {
        let input = Self::build_input_lines(req);
        debug!(bytes = input.len(), "claude-code chat input");

        let mut child = self.spawn_child(&req.model)
            .map_err(|e| ProviderError::Other(format!("spawn claude: {e}")))?;

        // 写入 stdin 后立即关闭，让 CC 知道输入已结束
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes()).await
                .map_err(|e| ProviderError::Other(format!("write stdin: {e}")))?;
        }

        let stdout = child.stdout.take()
            .ok_or_else(|| ProviderError::Other("no stdout".into()))?;
        let mut lines = BufReader::new(stdout).lines();

        let mut text     = String::new();
        let mut usage    = Usage::default();
        let mut stop     = StopReason::EndTurn;
        let mut model_id = req.model.clone();

        loop {
            let line = tokio::select! {
                _ = signal.cancelled() => {
                    let _ = child.kill().await;
                    return Err(ProviderError::Cancelled);
                }
                l = lines.next_line() => match l {
                    Ok(Some(l)) => l,
                    Ok(None)    => break,
                    Err(e)      => return Err(ProviderError::Other(e.to_string())),
                }
            };

            let cc: CcLine = match serde_json::from_str(&line) {
                Ok(v)  => v,
                Err(e) => { warn!(err=%e, "cc parse error"); continue; }
            };

            match cc.kind.as_str() {
                "stream_event" => {
                    if let Some(ev) = cc.event {
                        if ev.kind == "content_block_delta" {
                            if let Some(d) = ev.delta {
                                if d.kind == "text_delta" {
                                    if let Some(t) = d.text { text.push_str(&t); }
                                }
                            }
                        }
                    }
                }
                "result" => {
                    if cc.is_error == Some(true) {
                        let msg = cc.message.unwrap_or_else(|| "claude-code error".into());
                        return Err(ProviderError::Other(msg));
                    }
                    if let Some(u) = cc.usage { usage = Usage::from(u); }
                    if let Some(r) = cc.result { if text.is_empty() { text = r; } }
                    break;
                }
                "assistant" => {
                    // 完整 assistant 消息，可提取 model_id
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        if let Some(m) = v["message"]["model"].as_str() {
                            model_id = m.to_string();
                        }
                    }
                }
                _ => {}
            }
        }

        let _ = child.wait().await;

        let blocks = if text.is_empty() {
            vec![]
        } else {
            vec![ContentBlock::Text { text }]
        };
        Ok(ChatResponse {
            message:     Message::new(MessageRole::Assistant, blocks),
            stop_reason: stop,
            usage,
            model:       model_id,
        })
    }

    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError> {
        let input = Self::build_input_lines(req);
        debug!(bytes = input.len(), "claude-code stream input");

        let mut child = self.spawn_child(&req.model)
            .map_err(|e| ProviderError::Other(format!("spawn claude: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes()).await
                .map_err(|e| ProviderError::Other(format!("write stdin: {e}")))?;
        }

        let stdout = child.stdout.take()
            .ok_or_else(|| ProviderError::Other("no stdout".into()))?;

        let (tx, rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        tokio::spawn(run_cc_stream(stdout, child, signal, tx));
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(cc_model_list())
    }
}

// ── 后台流式任务 ─────────────────────────────────────────────────────────────

async fn run_cc_stream(
    stdout: tokio::process::ChildStdout,
    mut child: tokio::process::Child,
    signal: CancellationToken,
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
) {
    let mut lines   = BufReader::new(stdout).lines();
    let mut usage   = Usage::default();
    let mut stop    = StopReason::EndTurn;

    loop {
        let line = tokio::select! {
            _ = signal.cancelled() => {
                let _ = child.kill().await;
                let _ = tx.send(StreamEvent::Error("cancelled".into())).await;
                return;
            }
            l = lines.next_line() => match l {
                Ok(Some(l)) => l,
                Ok(None)    => break,
                Err(e)      => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                    return;
                }
            }
        };

        let cc: CcLine = match serde_json::from_str(&line) {
            Ok(v)  => v,
            Err(e) => { warn!(err=%e, "cc stream parse error"); continue; }
        };

        match cc.kind.as_str() {
            "stream_event" => {
                let Some(ev) = cc.event else { continue };
                match ev.kind.as_str() {
                    "content_block_delta" => {
                        let Some(d) = ev.delta else { continue };
                        if d.kind == "text_delta" {
                            if let Some(text) = d.text {
                                let _ = tx.send(StreamEvent::Chunk(
                                    StreamChunk::TextDelta { delta: text }
                                )).await;
                            }
                        }
                    }
                    "message_delta" => {
                        if let Some(sr) = ev.stop_reason {
                            stop = match sr.as_str() {
                                "end_turn"      => StopReason::EndTurn,
                                "tool_use"      => StopReason::ToolUse,
                                "max_tokens"    => StopReason::MaxTokens,
                                _               => StopReason::EndTurn,
                            };
                        }
                    }
                    _ => {}
                }
            }
            "result" => {
                if cc.is_error == Some(true) {
                    let msg = cc.message.unwrap_or_else(|| "claude-code error".into());
                    let _ = tx.send(StreamEvent::Error(msg)).await;
                    let _ = child.kill().await;
                    return;
                }
                if let Some(u) = cc.usage { usage = Usage::from(u); }
                break;
            }
            _ => {}
        }
    }

    let _ = tx.send(StreamEvent::Done { stop_reason: stop, usage }).await;
    let _ = child.wait().await;
}

// ── 模型列表（硬编码，CC 无 /v1/models 端点）────────────────────────────────

fn cc_model_list() -> Vec<ModelInfo> {
    [
        ("claude-opus-4-8",   "Claude Opus 4.8",   200_000, 32_000, true),
        ("claude-opus-4-7",   "Claude Opus 4.7",   200_000, 32_000, true),
        ("claude-opus-4-6",   "Claude Opus 4.6",   200_000, 32_000, true),
        ("claude-sonnet-4-6", "Claude Sonnet 4.6", 200_000, 64_000, true),
        ("claude-haiku-4-5-20251001", "Claude Haiku 4.5", 200_000, 8_000, false),
    ]
    .into_iter()
    .map(|(id, name, ctx, out, think)| ModelInfo {
        id:               id.to_string(),
        display_name:     name.to_string(),
        context_window:   ctx,
        max_output_tokens: out,
        supports_vision:  true,
        supports_thinking: think,
        supports_tools:   true,
    })
    .collect()
}
