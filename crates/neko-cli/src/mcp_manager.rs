// McpManager 的 neko-cli 实现：把 MCP server 的工具 + prompts 桥接进 nekocli。
// 这是 core 的 McpManager trait 的具体实现（依赖反转，打破 core→mcp 循环）。
//
// 双重职责：
// - 工具：MCP server 的 tools → ToolRegistry（LLM function call）
// - 技能：MCP server 的 prompts → SkillRegistry（用户 /slash 命令）
//   通过 neko_mcp::import_external_prompts 自动注入。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::Mutex;
use tracing::debug;

use neko_core::config::McpServerConfig;
use neko_core::runtime::McpManager;
use neko_core::skills::SkillRegistry;
use neko_core::tools::{Tool, ToolContext, ToolRegistry, ToolRegistryExt, ToolResult};

use neko_mcp::{import_external_prompts, McpClient, McpToolBridge, SseTransport, StdioTransport, Transport};

/// 持有所有活跃 MCP client，并实现工具加载/关闭 + prompts 注入。
pub struct CliMcpManager {
    clients: Mutex<HashMap<String, Arc<Mutex<McpClient>>>>,
    /// 共享的 SkillRegistry——MCP server 的 prompts 会注入这里，统一 /slash 发现。
    skills:  Arc<RwLock<SkillRegistry>>,
}

impl CliMcpManager {
    pub fn new(skills: Arc<RwLock<SkillRegistry>>) -> Self {
        Self { clients: Mutex::new(HashMap::new()), skills }
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

        // ── 工具：桥接进 ToolRegistry ──
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

        // ── 技能：把 server 的 prompts 注入 SkillRegistry（统一 /slash 发现）──
        {
            let client_guard = client_arc.lock().await;
            match import_external_prompts(&self.skills, &*client_guard).await {
                Ok(n) if n > 0 => debug!(server = %name, imported = n, "imported MCP prompts as skills"),
                Ok(_) => {}, // server 无 prompts（正常，不是所有 server 都提供）
                Err(e) => debug!(server = %name, err = %e, "failed to import MCP prompts as skills"),
            }
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
