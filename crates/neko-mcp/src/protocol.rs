use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── JSON-RPC 2.0 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id:      Option<Value>,
    pub method:  String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params:  Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self { jsonrpc: "2.0".into(), id: Some(id.into()), method: method.into(), params }
    }

    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self { jsonrpc: "2.0".into(), id: None, method: method.into(), params }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id:      Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result:  Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error:   Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code:    i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data:    Option<Value>,
}

// ── MCP Tool ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name:        String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

// ── MCP Prompt（2025-11-25 规范）──────────────────────────────────────────────

/// MCP Prompt 定义：用户可显式选择的模板化指令（slash 命令、skill）。
///
/// 语义上对应 nekocli 的 `Skill`——用户通过 `/skill-name` 触发。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title:       Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// `prompts/get` 返回的消息序列。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpGetPromptResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<McpPromptMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptMessage {
    pub role:    String, // "user" | "assistant"
    pub content: McpContent,
}

// ── MCP Resource（2025-11-25 规范）────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// 文本内容（与 blob 二选一）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// base64 编码的二进制内容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

// ── Content blocks（tools/prompts/resources 共用）─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum McpContent {
    Text     { text: String },
    Image    { mime_type: String, data: String },
    Audio    { mime_type: String, data: String },
    Resource { resource: McpResource },
}

// ── Request/Response wrappers（向后兼容）──────────────────────────────────────

pub struct McpRequest {
    pub tool:   String,
    pub params: Value,
}

pub struct McpResponse {
    pub content:  Vec<McpContent>,
    pub is_error: bool,
}

impl McpResponse {
    /// 提取所有文本内容，拼接为单个字符串（便捷方法）。
    pub fn text(&self) -> String {
        self.content.iter().filter_map(|c| match c {
            McpContent::Text { text } => Some(text.as_str()),
            _ => None,
        }).collect::<Vec<_>>().join("\n")
    }
}
