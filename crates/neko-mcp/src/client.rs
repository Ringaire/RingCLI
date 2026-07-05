use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::debug;
use uuid::Uuid;

use crate::error::McpError;
use crate::protocol::{
    JsonRpcRequest, JsonRpcResponse, McpGetPromptResult, McpPrompt, McpRequest, McpResponse, McpTool,
};
use crate::transport::Transport;

/// MCP 协议版本（2025-11-25 规范）。
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

const CALL_TIMEOUT_SECS: u64 = 60;

pub struct McpClient {
    transport:           Arc<Mutex<Box<dyn Transport>>>,
    tools:               Vec<McpTool>,
    prompts:             Vec<McpPrompt>,
    /// server 在 initialize 返回的 capabilities（决定哪些 list 方法可调）。
    server_capabilities: serde_json::Value,
}

impl McpClient {
    pub async fn new(transport: Box<dyn Transport>) -> Result<Self, McpError> {
        let mut client = Self {
            transport: Arc::new(Mutex::new(transport)),
            tools: Vec::new(),
            prompts: Vec::new(),
            server_capabilities: serde_json::Value::Null,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<(), McpError> {
        let req = JsonRpcRequest::new(
            Uuid::new_v4().to_string(),
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "neko", "version": env!("CARGO_PKG_VERSION") }
            })),
        );
        let resp = self.call_raw(req).await?;

        // 读 server capabilities（决定后续 list 方法是否可调）
        if let Some(result) = resp.result {
            self.server_capabilities = result.get("capabilities").cloned().unwrap_or_default();
        }

        let notify = JsonRpcRequest::notification("notifications/initialized", None);
        self.transport.lock().await.send(notify).await?;

        // 按 server capabilities 按需拉取（无 capability 声明时仍尝试 tools，向后兼容）
        if self.has_capability("tools") {
            self.refresh_tools().await?;
        } else {
            self.refresh_tools().await.ok(); // 容错：旧 server 可能不声明 capability
        }
        if self.has_capability("prompts") {
            self.refresh_prompts().await?;
        }
        Ok(())
    }

    /// server 是否声明了某项能力（"tools" / "prompts" / "resources"）。
    fn has_capability(&self, name: &str) -> bool {
        self.server_capabilities.get(name).is_some()
    }

    // ── Tools ─────────────────────────────────────────────────────────────────

    pub async fn refresh_tools(&mut self) -> Result<(), McpError> {
        let req = JsonRpcRequest::new(Uuid::new_v4().to_string(), "tools/list", None);
        let resp = self.call_raw(req).await?;
        if let Some(result) = resp.result {
            if let Ok(tools) = serde_json::from_value::<Vec<McpTool>>(result["tools"].clone()) {
                self.tools = tools;
                debug!(count = self.tools.len(), "MCP tools refreshed");
            }
        }
        Ok(())
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub async fn call(&self, req: &McpRequest) -> Result<McpResponse, McpError> {
        let rpc = JsonRpcRequest::new(
            Uuid::new_v4().to_string(),
            "tools/call",
            Some(serde_json::json!({ "name": req.tool, "arguments": req.params })),
        );
        let resp = self.call_raw(rpc).await?;

        if let Some(err) = resp.error {
            return Err(McpError::Rpc { code: err.code, message: err.message });
        }

        let result = resp.result.unwrap_or_default();
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let content = result["content"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
            .unwrap_or_default();
        Ok(McpResponse { content, is_error })
    }

    // ── Prompts（2025-11-25 规范）──────────────────────────────────────────────

    /// 拉取 `prompts/list`，缓存结果。仅在 server 声明 prompts 能力时调用。
    pub async fn refresh_prompts(&mut self) -> Result<(), McpError> {
        let req = JsonRpcRequest::new(Uuid::new_v4().to_string(), "prompts/list", None);
        let resp = self.call_raw(req).await?;
        if let Some(result) = resp.result {
            if let Ok(prompts) = serde_json::from_value::<Vec<McpPrompt>>(result["prompts"].clone()) {
                self.prompts = prompts;
                debug!(count = self.prompts.len(), "MCP prompts refreshed");
            }
        }
        Ok(())
    }

    pub fn prompts(&self) -> &[McpPrompt] {
        &self.prompts
    }

    /// 调用 `prompts/get`，返回展开后的消息序列。
    ///
    /// `arguments` 是 prompt 定义的参数键值对（可为空）。
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<&serde_json::Value>,
    ) -> Result<McpGetPromptResult, McpError> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments.unwrap_or(&serde_json::Value::Object(Default::default()))
        });
        let req = JsonRpcRequest::new(Uuid::new_v4().to_string(), "prompts/get", Some(params));
        let resp = self.call_raw(req).await?;

        if let Some(err) = resp.error {
            return Err(McpError::Rpc { code: err.code, message: err.message });
        }

        let result = resp.result.unwrap_or_default();
        serde_json::from_value(result).map_err(|e| McpError::Other(format!("parse prompt result: {e}")))
    }

    // ── Raw transport ─────────────────────────────────────────────────────────

    async fn call_raw(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let mut transport = self.transport.lock().await;
        transport.send(req).await?;
        tokio::time::timeout(
            Duration::from_secs(CALL_TIMEOUT_SECS),
            transport.recv(),
        )
        .await
        .map_err(|_| McpError::Timeout)?
    }
}
