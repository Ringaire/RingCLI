// McpManager 的 neko-cli 实现：把 MCP server 的工具桥接进 ToolRegistry。
// 这是 core 的 McpManager trait 的具体实现（依赖反转，打破 core→mcp 循环）。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::debug;

use neko_core::config::McpServerConfig;
use neko_core::runtime::McpManager;
use neko_core::tools::{Tool, ToolContext, ToolRegistry, ToolRegistryExt, ToolResult};

use neko_mcp::{McpClient, McpToolBridge, SseTransport, StdioTransport, Transport};

/// 持有所有活跃 MCP client，并实现工具加载/关闭。
pub struct CliMcpManager {
    clients: Mutex<HashMap<String, Arc<Mutex<McpClient>>>>,
}

impl CliMcpManager {
    pub fn new() -> Self {
        Self { clients: Mutex::new(HashMap::new()) }
    }
}

impl Default for CliMcpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl McpManager for CliMcpManager {
    async fn load(
        &self,
        name:  &str,
        cfg:   &McpServerConfig,
        tools: Arc<dyn ToolRegistry>,
    ) -> Result<Vec<String>, String> {
        let transport: Box<dyn Transport> = match cfg {
            McpServerConfig::Stdio { command, args, env } => {
                let env_vec: Vec<(String, String)> =
                    env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let t = StdioTransport::spawn(command, args, &env_vec).await
                    .map_err(|e| format!("spawn stdio server '{name}': {e}"))?;
                Box::new(t)
            }
            McpServerConfig::Sse { url, headers } => {
                let t = SseTransport::new(url.clone(), headers.clone())
                    .map_err(|e| format!("build SSE transport for '{name}': {e}"))?;
                Box::new(t)
            }
        };

        let client = McpClient::new(transport).await
            .map_err(|e| format!("initialize MCP client '{name}': {e}"))?;

        let mcp_tools = client.tools().to_vec();
        let client_arc = Arc::new(Mutex::new(client));

        let mut registered = Vec::new();
        for mcp_tool in &mcp_tools {
            // 命名空间化：mcp__{server}__{tool}，避免与内置工具冲突
            let bridged_name = format!("mcp__{}__{}", name, mcp_tool.name);
            let description = mcp_tool.description.clone()
                .unwrap_or_else(|| format!("MCP tool {} from server {}", mcp_tool.name, name));

            let bridge = McpToolBridge::new(
                client_arc.clone(),
                mcp_tool.name.clone(),  // 调用时用原始名
                description,
                mcp_tool.input_schema.clone(),
            );
            let namespaced = NamespacedTool { inner: bridge, name: bridged_name.clone() };
            tools.register(namespaced);
            registered.push(bridged_name.clone());
            debug!(server = %name, tool = %bridged_name, "registered MCP tool");
        }

        self.clients.lock().await.insert(name.to_string(), client_arc);
        Ok(registered)
    }

    async fn close(&self, name: &str) {
        // 移除 client：Arc 引用归零后 McpClient 析构，stdio 子进程随之终止
        self.clients.lock().await.remove(name);
        debug!(server = %name, "MCP client closed");
    }
}

/// 用命名空间名覆盖内层工具 name() 的包装器。
struct NamespacedTool {
    inner: McpToolBridge,
    name:  String,
}

#[async_trait]
impl Tool for NamespacedTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { self.inner.description() }
    fn input_schema(&self) -> serde_json::Value { self.inner.input_schema() }
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        self.inner.execute(input, ctx).await
    }
}
