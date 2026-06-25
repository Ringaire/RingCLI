use async_trait::async_trait;
use futures_util::Stream;
use neko_core::tools::Message;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::error::ProviderError;

// ── 共享 HTTP 客户端构建 ──────────────────────────────────────────────────────

/// 默认连接超时（秒）
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// 默认单次响应的最大输出 token（provider 与 agent executor 共用）。
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 8192;

/// 默认 extended-thinking token 预算。
pub const DEFAULT_THINKING_BUDGET: u32 = 8000;

/// 未知模型的默认上下文窗口（token）。
pub const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;

/// HTTP 响应错误归一化（所有 provider 共用）：429 → RateLimit，401 → Auth，其余 → Http。
pub async fn check_response_error(
    resp: reqwest::Response,
) -> Result<reqwest::Response, ProviderError> {
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        if status == 429 { return Err(ProviderError::RateLimit { retry_after_secs: None }); }
        if status == 401 { return Err(ProviderError::Auth(body)); }
        return Err(ProviderError::Http { status, body });
    }
    Ok(resp)
}

/// 构建带可选代理的 reqwest 客户端。
/// proxy 解析失败时回退为无代理（不静默 panic）。
pub fn build_http_client(proxy: Option<&str>, connect_timeout_secs: u64) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(connect_timeout_secs))
        .user_agent(concat!("neko/", env!("CARGO_PKG_VERSION")));

    if let Some(proxy_url) = proxy {
        if !proxy_url.trim().is_empty() {
            match reqwest::Proxy::all(proxy_url) {
                Ok(p)  => builder = builder.proxy(p),
                Err(e) => tracing::warn!(proxy = %proxy_url, err = %e, "invalid proxy URL, ignoring"),
            }
        }
    }

    builder.build().unwrap_or_else(|e| {
        tracing::warn!(err = %e, "failed to build http client with options, falling back to default");
        reqwest::Client::new()
    })
}

// ── 工具定义（传给 LLM）───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name:        String,
    pub description: String,
    #[serde(rename = "input_schema")]
    pub input_schema: serde_json::Value,
}

// ── 请求 / 响应 ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model:        String,
    pub messages:     Vec<Message>,
    pub system:       Option<String>,
    pub tools:        Vec<ToolDef>,
    pub max_tokens:   u32,
    pub temperature:  Option<f32>,
    pub top_p:        Option<f32>,
    pub stop:         Vec<String>,
    pub extended_thinking: bool,
    pub thinking_budget:   Option<u32>,
    /// reasoning effort 级别（OpenAI o-series / 智谱 GLM-5+）。
    pub reasoning_effort:  Option<String>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model:          model.into(),
            messages,
            system:         None,
            tools:          Vec::new(),
            max_tokens:     DEFAULT_MAX_OUTPUT_TOKENS,
            temperature:    None,
            top_p:          None,
            stop:           Vec::new(),
            extended_thinking: false,
            thinking_budget:   None,
            reasoning_effort:  None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens:             u64,
    pub output_tokens:            u64,
    pub cache_creation_tokens:    u64,
    pub cache_read_tokens:        u64,
}

impl std::ops::Add for Usage {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            input_tokens:          self.input_tokens  + rhs.input_tokens,
            output_tokens:         self.output_tokens + rhs.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens + rhs.cache_creation_tokens,
            cache_read_tokens:     self.cache_read_tokens     + rhs.cache_read_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Error,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub message:     neko_core::tools::Message,
    pub stop_reason: StopReason,
    pub usage:       Usage,
    pub model:       String,
}

// ── 流式事件 ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum StreamChunk {
    ThinkingDelta { delta: String },
    ThinkingDone  { full: String },
    TextDelta     { delta: String },
    TextDone      { full: String },
    ToolCallStart { call_id: String, tool_name: String },
    ToolCallInput { call_id: String, delta: String },
    ToolCallDone  { call_id: String, full_input: serde_json::Value },
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Chunk(StreamChunk),
    Done { stop_reason: StopReason, usage: Usage },
    Error(String),
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;

// ── 模型信息 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id:               String,
    pub display_name:     String,
    pub context_window:   u64,
    pub max_output_tokens: u64,
    pub supports_vision:  bool,
    pub supports_thinking: bool,
    pub supports_tools:   bool,
}

// ── Provider trait ────────────────────────────────────────────────────────────

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError>;
    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    fn default_model(&self) -> &str;
}
