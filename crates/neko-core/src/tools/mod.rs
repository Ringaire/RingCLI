use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ── 内容块（消息内容的原子单元）─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolInput")]
        tool_input: serde_json::Value,
    },
    ToolResult {
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        #[serde(rename = "toolResult")]
        tool_result: serde_json::Value,
        #[serde(rename = "isError", default)]
        is_error: bool,
    },
    /// 图片块（base64 编码）。
    Image {
        media_type: String,
        data: String,
    },
}

// ── 消息角色 ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    ToolResult,
}

// ── 消息 ─────────────────────────────────────────────────────────────────────

/// token 用量。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens:       u64,
    #[serde(default)]
    pub output_tokens:      u64,
    #[serde(default)]
    pub cache_read_tokens:  u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    pub ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model:       Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage:       Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

impl Message {
    pub fn new(role: MessageRole, content: Vec<ContentBlock>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            content,
            ts: chrono::Utc::now().timestamp_millis(),
            model: None,
            usage: None,
            stop_reason: None,
        }
    }

    pub fn user_text(text: impl Into<String>) -> Self {
        Self::new(MessageRole::User, vec![ContentBlock::Text { text: text.into() }])
    }
}

// ── 工具执行结果 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContent {
    Text { text: String },
    Image { media_type: String, data: String },
    Resource {
        uri: String,
        mime_type: Option<String>,
        text: Option<String>,
    },
}

#[derive(Debug)]
pub enum ToolResult {
    Ok {
        content: Vec<ToolResultContent>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    },
    Err {
        error: String,
        code: Option<String>,
    },
}

impl ToolResult {
    pub fn ok_text(text: impl Into<String>) -> Self {
        Self::Ok {
            content: vec![ToolResultContent::Text { text: text.into() }],
            metadata: None,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self::Err { error: error.into(), code: None }
    }

    pub fn err_code(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self::Err { error: error.into(), code: Some(code.into()) }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    pub fn text(&self) -> String {
        match self {
            Self::Ok { content, .. } => content
                .iter()
                .filter_map(|c| if let ToolResultContent::Text { text } = c { Some(text.as_str()) } else { None })
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Err { error, .. } => error.clone(),
        }
    }
}

// ── 工具上下文（执行时传递）──────────────────────────────────────────────────

pub struct ToolContext {
    pub cwd: std::path::PathBuf,
    pub session_id: Uuid,
    pub signal: tokio_util::sync::CancellationToken,
    pub emit: Arc<dyn Fn(String, serde_json::Value) + Send + Sync>,
    pub env: HashMap<String, String>,
}

// ── Tool trait ────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn prompt(&self) -> Option<&str> { None }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> ToolResult;
}

// ── 工具注册表 trait ──────────────────────────────────────────────────────────

/// 工具注册表抽象。trait 化以支持装饰器（Augmented/Sub 注册表），
/// 用于编排器注入 spawn_agent、子 agent 剥离工具等。
/// 方法用 `&self` + 内部可变性，便于在 `Arc<dyn ToolRegistry>` 上共享。
pub trait ToolRegistry: Send + Sync {
    /// 注册一个已装箱的工具。
    fn register_arc(&self, tool: Arc<dyn Tool>);
    /// 按名注销。
    fn unregister(&self, name: &str);
    /// 按名取工具。
    fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;
    /// 列出全部工具。
    fn list(&self) -> Vec<Arc<dyn Tool>>;
}

/// 便捷扩展：泛型 `register`（不能放进 trait 体内，否则破坏对象安全）。
pub trait ToolRegistryExt: ToolRegistry {
    fn register(&self, tool: impl Tool + 'static) {
        self.register_arc(Arc::new(tool));
    }
}
impl<T: ToolRegistry + ?Sized> ToolRegistryExt for T {}

/// 默认实现：基于 `RwLock<HashMap>` 的线程安全注册表。
#[derive(Default)]
pub struct DefaultToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl DefaultToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolRegistry for DefaultToolRegistry {
    fn register_arc(&self, tool: Arc<dyn Tool>) {
        self.tools.write().insert(tool.name().to_string(), tool);
    }

    fn unregister(&self, name: &str) {
        self.tools.write().remove(name);
    }

    fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().get(name).cloned()
    }

    fn list(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.read().values().cloned().collect()
    }
}

// ── 装饰器：增补注册表（在内层之上叠加额外工具，如 spawn_agent）─────────────────

/// 在一个已有注册表之上叠加额外工具的视图。
/// `get`/`list` 优先返回额外工具；`register`/`unregister` 透传给内层。
pub struct AugmentedToolRegistry {
    inner:  Arc<dyn ToolRegistry>,
    extras: HashMap<String, Arc<dyn Tool>>,
}

impl AugmentedToolRegistry {
    pub fn new(inner: Arc<dyn ToolRegistry>, extras: Vec<Arc<dyn Tool>>) -> Self {
        let extras = extras.into_iter().map(|t| (t.name().to_string(), t)).collect();
        Self { inner, extras }
    }
}

impl ToolRegistry for AugmentedToolRegistry {
    fn register_arc(&self, tool: Arc<dyn Tool>) {
        self.inner.register_arc(tool);
    }

    fn unregister(&self, name: &str) {
        self.inner.unregister(name);
    }

    fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.extras.get(name).cloned().or_else(|| self.inner.get(name))
    }

    fn list(&self) -> Vec<Arc<dyn Tool>> {
        let mut out = self.inner.list();
        out.extend(self.extras.values().cloned());
        out
    }
}

// ── 装饰器：过滤注册表（从内层视图中剥离指定工具，如子 agent 去掉 spawn_agent）──

/// 从内层注册表视图中排除一组工具名。
pub struct SubToolRegistry {
    inner:   Arc<dyn ToolRegistry>,
    exclude: std::collections::HashSet<String>,
}

impl SubToolRegistry {
    pub fn new(inner: Arc<dyn ToolRegistry>, exclude: impl IntoIterator<Item = String>) -> Self {
        Self { inner, exclude: exclude.into_iter().collect() }
    }
}

impl ToolRegistry for SubToolRegistry {
    fn register_arc(&self, tool: Arc<dyn Tool>) {
        self.inner.register_arc(tool);
    }

    fn unregister(&self, name: &str) {
        self.inner.unregister(name);
    }

    fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if self.exclude.contains(name) {
            None
        } else {
            self.inner.get(name)
        }
    }

    fn list(&self) -> Vec<Arc<dyn Tool>> {
        self.inner.list().into_iter().filter(|t| !self.exclude.contains(t.name())).collect()
    }
}
