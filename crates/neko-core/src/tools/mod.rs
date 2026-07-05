use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ── 子模块 ────────────────────────────────────────────────────────────────────

pub mod builtin;
pub mod hybrid;

pub use builtin::BuiltinToolKind;
pub use hybrid::HybridToolRegistry;

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

// ── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── 测试工具实现 ──────────────────────────────────────────────────────────

    struct MockTool {
        name: String,
        description: String,
    }

    impl MockTool {
        fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                description: description.into(),
            }
        }
    }

    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }

        async fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::ok_text("mock result")
        }
    }

    // ── ContentBlock 测试 ─────────────────────────────────────────────────────

    #[test]
    fn test_content_block_text_serialization() {
        let block = ContentBlock::Text {
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Hello\""));

        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        match deserialized {
            ContentBlock::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_content_block_tool_use_serialization() {
        let block = ContentBlock::ToolUse {
            tool_use_id: "tu_123".to_string(),
            tool_name: "test_tool".to_string(),
            tool_input: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"tool_use\""));
        assert!(json.contains("\"toolUseId\":\"tu_123\""));
        assert!(json.contains("\"toolName\":\"test_tool\""));

        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        match deserialized {
            ContentBlock::ToolUse { tool_use_id, tool_name, .. } => {
                assert_eq!(tool_use_id, "tu_123");
                assert_eq!(tool_name, "test_tool");
            }
            _ => panic!("Expected ToolUse variant"),
        }
    }

    #[test]
    fn test_content_block_tool_result_serialization() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_456".to_string(),
            tool_result: serde_json::json!({"result": "ok"}),
            is_error: false,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"toolUseId\":\"tu_456\""));

        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        match deserialized {
            ContentBlock::ToolResult { tool_use_id, is_error, .. } => {
                assert_eq!(tool_use_id, "tu_456");
                assert!(!is_error);
            }
            _ => panic!("Expected ToolResult variant"),
        }
    }

    #[test]
    fn test_content_block_image_serialization() {
        let block = ContentBlock::Image {
            media_type: "image/png".to_string(),
            data: "base64data".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        assert!(json.contains("\"media_type\":\"image/png\""));

        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        match deserialized {
            ContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "base64data");
            }
            _ => panic!("Expected Image variant"),
        }
    }

    // ── MessageRole 测试 ──────────────────────────────────────────────────────

    #[test]
    fn test_message_role_serialization() {
        let user = MessageRole::User;
        assert_eq!(serde_json::to_string(&user).unwrap(), "\"user\"");

        let assistant = MessageRole::Assistant;
        assert_eq!(serde_json::to_string(&assistant).unwrap(), "\"assistant\"");

        let tool_result = MessageRole::ToolResult;
        assert_eq!(serde_json::to_string(&tool_result).unwrap(), "\"tool_result\"");
    }

    #[test]
    fn test_message_role_deserialization() {
        let user: MessageRole = serde_json::from_str("\"user\"").unwrap();
        assert_eq!(user, MessageRole::User);

        let assistant: MessageRole = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(assistant, MessageRole::Assistant);

        let tool_result: MessageRole = serde_json::from_str("\"tool_result\"").unwrap();
        assert_eq!(tool_result, MessageRole::ToolResult);
    }

    // ── Message 测试 ──────────────────────────────────────────────────────────

    #[test]
    fn test_message_new() {
        let msg = Message::new(
            MessageRole::User,
            vec![ContentBlock::Text { text: "test".to_string() }],
        );
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content.len(), 1);
        assert!(msg.model.is_none());
        assert!(msg.usage.is_none());
        assert!(msg.stop_reason.is_none());
    }

    #[test]
    fn test_message_user_text() {
        let msg = Message::user_text("Hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message::user_text("test");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, MessageRole::User);
        assert_eq!(deserialized.content.len(), 1);
    }

    // ── Usage 测试 ────────────────────────────────────────────────────────────

    #[test]
    fn test_usage_default() {
        let usage = Usage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 0);
    }

    #[test]
    fn test_usage_serialization() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_write_tokens: 5,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.input_tokens, 100);
        assert_eq!(deserialized.output_tokens, 50);
        assert_eq!(deserialized.cache_read_tokens, 10);
        assert_eq!(deserialized.cache_write_tokens, 5);
    }

    // ── ToolResultContent 测试 ────────────────────────────────────────────────

    #[test]
    fn test_tool_result_content_text_serialization() {
        let content = ToolResultContent::Text {
            text: "result".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"result\""));
    }

    #[test]
    fn test_tool_result_content_image_serialization() {
        let content = ToolResultContent::Image {
            media_type: "image/jpeg".to_string(),
            data: "base64".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        assert!(json.contains("\"media_type\":\"image/jpeg\""));
    }

    #[test]
    fn test_tool_result_content_resource_serialization() {
        let content = ToolResultContent::Resource {
            uri: "file:///test".to_string(),
            mime_type: Some("text/plain".to_string()),
            text: Some("content".to_string()),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"resource\""));
        assert!(json.contains("\"uri\":\"file:///test\""));
    }

    // ── ToolResult 测试 ───────────────────────────────────────────────────────

    #[test]
    fn test_tool_result_ok_text() {
        let result = ToolResult::ok_text("success");
        assert!(result.is_ok());
        assert_eq!(result.text(), "success");
    }

    #[test]
    fn test_tool_result_err() {
        let result = ToolResult::err("failed");
        assert!(!result.is_ok());
        assert_eq!(result.text(), "failed");
    }

    #[test]
    fn test_tool_result_err_code() {
        let result = ToolResult::err_code("failed", "ERR_CODE");
        assert!(!result.is_ok());
        assert_eq!(result.text(), "failed");
        match result {
            ToolResult::Err { error, code } => {
                assert_eq!(error, "failed");
                assert_eq!(code, Some("ERR_CODE".to_string()));
            }
            _ => panic!("Expected Err variant"),
        }
    }

    #[test]
    fn test_tool_result_text_multiple_content() {
        let result = ToolResult::Ok {
            content: vec![
                ToolResultContent::Text { text: "line1".to_string() },
                ToolResultContent::Text { text: "line2".to_string() },
            ],
            metadata: None,
        };
        assert_eq!(result.text(), "line1\nline2");
    }

    #[test]
    fn test_tool_result_text_mixed_content() {
        let result = ToolResult::Ok {
            content: vec![
                ToolResultContent::Text { text: "text1".to_string() },
                ToolResultContent::Image {
                    media_type: "image/png".to_string(),
                    data: "data".to_string(),
                },
                ToolResultContent::Text { text: "text2".to_string() },
            ],
            metadata: None,
        };
        assert_eq!(result.text(), "text1\ntext2");
    }

    // ── DefaultToolRegistry 测试 ──────────────────────────────────────────────

    #[test]
    fn test_default_tool_registry_new() {
        let registry = DefaultToolRegistry::new();
        assert_eq!(registry.list().len(), 0);
    }

    #[test]
    fn test_default_tool_registry_register_and_get() {
        let registry = DefaultToolRegistry::new();
        let tool = Arc::new(MockTool::new("test_tool", "A test tool"));

        registry.register_arc(tool.clone());

        let retrieved = registry.get("test_tool");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "test_tool");
    }

    #[test]
    fn test_default_tool_registry_get_nonexistent() {
        let registry = DefaultToolRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_default_tool_registry_list() {
        let registry = DefaultToolRegistry::new();
        registry.register_arc(Arc::new(MockTool::new("tool1", "Tool 1")));
        registry.register_arc(Arc::new(MockTool::new("tool2", "Tool 2")));
        registry.register_arc(Arc::new(MockTool::new("tool3", "Tool 3")));

        let tools = registry.list();
        assert_eq!(tools.len(), 3);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"tool1"));
        assert!(names.contains(&"tool2"));
        assert!(names.contains(&"tool3"));
    }

    #[test]
    fn test_default_tool_registry_unregister() {
        let registry = DefaultToolRegistry::new();
        registry.register_arc(Arc::new(MockTool::new("tool1", "Tool 1")));
        registry.register_arc(Arc::new(MockTool::new("tool2", "Tool 2")));

        assert_eq!(registry.list().len(), 2);

        registry.unregister("tool1");
        assert_eq!(registry.list().len(), 1);
        assert!(registry.get("tool1").is_none());
        assert!(registry.get("tool2").is_some());
    }

    #[test]
    fn test_default_tool_registry_unregister_nonexistent() {
        let registry = DefaultToolRegistry::new();
        registry.register_arc(Arc::new(MockTool::new("tool1", "Tool 1")));

        registry.unregister("nonexistent");
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn test_default_tool_registry_duplicate_register() {
        let registry = DefaultToolRegistry::new();
        registry.register_arc(Arc::new(MockTool::new("tool1", "First")));
        registry.register_arc(Arc::new(MockTool::new("tool1", "Second")));

        let tools = registry.list();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].description(), "Second");
    }

    // ── ToolRegistryExt 测试 ──────────────────────────────────────────────────

    #[test]
    fn test_tool_registry_ext_register() {
        let registry = DefaultToolRegistry::new();
        let tool = MockTool::new("ext_tool", "Extension test");

        registry.register(tool);

        assert!(registry.get("ext_tool").is_some());
        assert_eq!(registry.get("ext_tool").unwrap().description(), "Extension test");
    }

    #[test]
    fn test_tool_registry_ext_register_arc() {
        let registry = DefaultToolRegistry::new();
        let tool = Arc::new(MockTool::new("arc_tool", "Arc test"));

        registry.register_arc(tool);

        assert!(registry.get("arc_tool").is_some());
    }

    // ── AugmentedToolRegistry 测试 ────────────────────────────────────────────

    #[test]
    fn test_augmented_tool_registry_priority() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("tool1", "Base tool"));
        base.register(MockTool::new("tool2", "Base tool 2"));

        let extra = Arc::new(MockTool::new("tool1", "Extra tool"));
        let augmented = AugmentedToolRegistry::new(base, vec![extra]);

        let retrieved = augmented.get("tool1").unwrap();
        assert_eq!(retrieved.description(), "Extra tool");
    }

    #[test]
    fn test_augmented_tool_registry_fallback_to_base() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("base_tool", "Base tool"));

        let extra = Arc::new(MockTool::new("extra_tool", "Extra tool"));
        let augmented = AugmentedToolRegistry::new(base, vec![extra]);

        assert_eq!(augmented.get("base_tool").unwrap().description(), "Base tool");
        assert_eq!(augmented.get("extra_tool").unwrap().description(), "Extra tool");
    }

    #[test]
    fn test_augmented_tool_registry_list_merge() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("base1", "Base 1"));
        base.register(MockTool::new("base2", "Base 2"));

        let extra1 = Arc::new(MockTool::new("extra1", "Extra 1"));
        let extra2 = Arc::new(MockTool::new("extra2", "Extra 2"));
        let augmented = AugmentedToolRegistry::new(base, vec![extra1, extra2]);

        let tools = augmented.list();
        assert_eq!(tools.len(), 4);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"base1"));
        assert!(names.contains(&"base2"));
        assert!(names.contains(&"extra1"));
        assert!(names.contains(&"extra2"));
    }

    #[test]
    fn test_augmented_tool_registry_register_passthrough() {
        let base = Arc::new(DefaultToolRegistry::new());
        let augmented = AugmentedToolRegistry::new(base.clone(), vec![]);

        augmented.register(MockTool::new("new_tool", "New tool"));

        assert!(base.get("new_tool").is_some());
        assert!(augmented.get("new_tool").is_some());
    }

    #[test]
    fn test_augmented_tool_registry_unregister_passthrough() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("tool1", "Tool 1"));

        let augmented = AugmentedToolRegistry::new(base.clone(), vec![]);

        augmented.unregister("tool1");

        assert!(base.get("tool1").is_none());
        assert!(augmented.get("tool1").is_none());
    }

    #[test]
    fn test_augmented_tool_registry_cannot_unregister_extra() {
        let base = Arc::new(DefaultToolRegistry::new());
        let extra = Arc::new(MockTool::new("extra_tool", "Extra"));
        let augmented = AugmentedToolRegistry::new(base, vec![extra]);

        augmented.unregister("extra_tool");

        assert!(augmented.get("extra_tool").is_some());
    }

    // ── SubToolRegistry 测试 ──────────────────────────────────────────────────

    #[test]
    fn test_sub_tool_registry_exclude_tool() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("tool1", "Tool 1"));
        base.register(MockTool::new("tool2", "Tool 2"));
        base.register(MockTool::new("tool3", "Tool 3"));

        let sub = SubToolRegistry::new(base, vec!["tool2".to_string()]);

        assert!(sub.get("tool1").is_some());
        assert!(sub.get("tool2").is_none());
        assert!(sub.get("tool3").is_some());
    }

    #[test]
    fn test_sub_tool_registry_list_filtered() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("tool1", "Tool 1"));
        base.register(MockTool::new("tool2", "Tool 2"));
        base.register(MockTool::new("tool3", "Tool 3"));

        let sub = SubToolRegistry::new(
            base,
            vec!["tool1".to_string(), "tool3".to_string()],
        );

        let tools = sub.list();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "tool2");
    }

    #[test]
    fn test_sub_tool_registry_register_passthrough() {
        let base = Arc::new(DefaultToolRegistry::new());
        let sub = SubToolRegistry::new(base.clone(), vec!["excluded".to_string()]);

        sub.register(MockTool::new("new_tool", "New tool"));

        assert!(base.get("new_tool").is_some());
        assert!(sub.get("new_tool").is_some());
    }

    #[test]
    fn test_sub_tool_registry_unregister_passthrough() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("tool1", "Tool 1"));

        let sub = SubToolRegistry::new(base.clone(), vec![]);

        sub.unregister("tool1");

        assert!(base.get("tool1").is_none());
        assert!(sub.get("tool1").is_none());
    }

    #[test]
    fn test_sub_tool_registry_empty_exclude_list() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("tool1", "Tool 1"));
        base.register(MockTool::new("tool2", "Tool 2"));

        let sub = SubToolRegistry::new(base, Vec::<String>::new());

        let tools = sub.list();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_sub_tool_registry_exclude_nonexistent() {
        let base = Arc::new(DefaultToolRegistry::new());
        base.register(MockTool::new("tool1", "Tool 1"));

        let sub = SubToolRegistry::new(base, vec!["nonexistent".to_string()]);

        let tools = sub.list();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "tool1");
    }
}
