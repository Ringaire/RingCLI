// RingRuntime：会话级运行时管理器。
//
// 负责工具注册表、事件总线、技能注册表，以及 MCP 服务器的动态加载/卸载与
// 配置热重载。
//
// 注：core 不能依赖 ring-mcp（否则 mcp→core→mcp 循环）。因此 MCP 的具体加载
// 通过 `McpManager` trait 做依赖反转 —— 由上层（ring-cli）注入实现。这是 Rust
// 中对 bun 版「动态 import('@ringcode/mcp')」那一 hack 的等价、且类型安全的写法。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tracing::{debug, warn};

use crate::config::McpServerConfig;
use crate::events::EventBus;
use crate::skills::SkillRegistry;
use crate::tools::{HybridToolRegistry, ToolRegistry};

/// MCP 服务器管理器：由上层提供实现，打破 core→mcp 循环依赖。
#[async_trait]
pub trait McpManager: Send + Sync {
    /// 加载一个 MCP server，把其工具注册进 `tools`，返回注册的（命名空间化）工具名。
    async fn load(
        &self,
        name:  &str,
        cfg:   &McpServerConfig,
        tools: Arc<dyn ToolRegistry>,
    ) -> Result<Vec<String>, String>;

    /// 关闭某 server（终止子进程 / 断开连接）。
    async fn close(&self, name: &str);
}

/// 单个 MCP server 的运行态。
struct McpServerState {
    cfg:        McpServerConfig,
    tool_names: Vec<String>,
}

pub struct RingRuntime {
    pub bus:    EventBus,
    pub tools:  Arc<HybridToolRegistry>,
    pub skills: Arc<RwLock<SkillRegistry>>,
    mcp_manager: RwLock<Option<Arc<dyn McpManager>>>,
    mcp_servers: RwLock<HashMap<String, McpServerState>>,
}

impl RingRuntime {
    pub fn new() -> Self {
        Self {
            bus:         EventBus::new(),
            tools:       Arc::new(HybridToolRegistry::new()),
            skills:      Arc::new(RwLock::new(SkillRegistry::new())),
            mcp_manager: RwLock::new(None),
            mcp_servers: RwLock::new(HashMap::new()),
        }
    }

    /// 使用预初始化的工具注册表创建运行时。
    ///
    /// 用于注入已注册内置工具的 `HybridToolRegistry`，
    /// 避免在创建后再逐一注册工具。
    pub fn new_with_tools(tools: HybridToolRegistry) -> Self {
        Self {
            bus:         EventBus::new(),
            tools:       Arc::new(tools),
            skills:      Arc::new(RwLock::new(SkillRegistry::new())),
            mcp_manager: RwLock::new(None),
            mcp_servers: RwLock::new(HashMap::new()),
        }
    }

    /// 注入 MCP 管理器实现。
    pub fn set_mcp_manager(&self, manager: Arc<dyn McpManager>) {
        *self.mcp_manager.write() = Some(manager);
    }

    /// `tools` 作为 trait 对象（供 executor / 桥接使用）。
    pub fn tools_dyn(&self) -> Arc<dyn ToolRegistry> {
        self.tools.clone()
    }

    fn manager(&self) -> Option<Arc<dyn McpManager>> {
        self.mcp_manager.read().clone()
    }

    /// 当前已加载的 MCP server 名称。
    pub fn mcp_server_names(&self) -> Vec<String> {
        self.mcp_servers.read().keys().cloned().collect()
    }

    // ── MCP 动态管理 ─────────────────────────────────────────────────────────

    /// 加载（或重载）一个 MCP server。
    pub async fn load_mcp_server(&self, name: &str, cfg: McpServerConfig) -> Result<(), String> {
        // 已存在则先卸载（重载语义）
        if self.mcp_servers.read().contains_key(name) {
            self.unload_mcp_server(name).await;
        }

        let Some(manager) = self.manager() else {
            return Err("no MCP manager registered".to_string());
        };

        let tool_names = manager.load(name, &cfg, self.tools_dyn()).await?;
        debug!(server = %name, tools = tool_names.len(), "MCP server loaded");
        self.mcp_servers.write().insert(name.to_string(), McpServerState { cfg, tool_names });
        Ok(())
    }

    /// 卸载一个 MCP server：注销其工具并关闭连接。
    pub async fn unload_mcp_server(&self, name: &str) {
        let state = self.mcp_servers.write().remove(name);
        if let Some(state) = state {
            for t in &state.tool_names {
                self.tools.unregister(t);
            }
            if let Some(manager) = self.manager() {
                manager.close(name).await;
            }
            debug!(server = %name, "MCP server unloaded");
        }
    }

    /// 按新的 mcp_servers 配置做增量 diff：卸载已移除的、加载/重载新增或变更的。
    pub async fn apply_mcp_config(&self, desired: &HashMap<String, McpServerConfig>) {
        // 卸载不在新配置中的
        let current: Vec<String> = self.mcp_server_names();
        for name in &current {
            if !desired.contains_key(name) {
                self.unload_mcp_server(name).await;
            }
        }
        // 加载 / 重载新配置中的（配置变更才重载）
        for (name, cfg) in desired {
            let changed = {
                let servers = self.mcp_servers.read();
                match servers.get(name) {
                    Some(state) => !mcp_cfg_eq(&state.cfg, cfg),
                    None        => true,
                }
            };
            if changed {
                if let Err(e) = self.load_mcp_server(name, cfg.clone()).await {
                    warn!(server = %name, err = %e, "failed to (re)load MCP server");
                }
            }
        }
    }

    /// 释放：卸载全部 MCP server。
    pub async fn dispose(&self) {
        for name in self.mcp_server_names() {
            self.unload_mcp_server(&name).await;
        }
    }
}

impl Default for RingRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// 比较两个 MCP server 配置是否等价（用于判断是否需要重载）。
fn mcp_cfg_eq(a: &McpServerConfig, b: &McpServerConfig) -> bool {
    use McpServerConfig::*;
    match (a, b) {
        (Stdio { command: c1, args: a1, env: e1 }, Stdio { command: c2, args: a2, env: e2 }) => {
            c1 == c2 && a1 == a2 && e1 == e2
        }
        (Sse { url: u1, headers: h1 }, Sse { url: u2, headers: h2 }) => u1 == u2 && h1 == h2,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::HybridToolRegistry;
    use std::collections::HashMap;

    #[test]
    fn test_ring_runtime_new() {
        let runtime = RingRuntime::new();
        assert!(runtime.tools.get("bash").is_none()); // 空注册表
    }

    #[test]
    fn test_ring_runtime_new_with_tools() {
        let registry = HybridToolRegistry::new();
        let runtime = RingRuntime::new_with_tools(registry);
        assert!(runtime.tools_dyn().get("bash").is_none());
    }

    #[test]
    fn test_ring_runtime_tools_dyn() {
        let runtime = RingRuntime::new();
        let trait_obj = runtime.tools_dyn();
        assert!(trait_obj.get("bash").is_none());
    }

    #[test]
    fn test_ring_runtime_default() {
        let runtime = RingRuntime::default();
        assert!(runtime.mcp_manager.read().is_none());
    }

    #[test]
    fn test_mcp_cfg_eq_stdio_same() {
        let cfg1 = McpServerConfig::Stdio {
            command: "test".to_string(),
            args: vec!["arg1".to_string()],
            env: HashMap::new(),
        };
        let cfg2 = McpServerConfig::Stdio {
            command: "test".to_string(),
            args: vec!["arg1".to_string()],
            env: HashMap::new(),
        };
        assert!(mcp_cfg_eq(&cfg1, &cfg2));
    }

    #[test]
    fn test_mcp_cfg_eq_stdio_different_command() {
        let cfg1 = McpServerConfig::Stdio {
            command: "test1".to_string(),
            args: vec![],
            env: HashMap::new(),
        };
        let cfg2 = McpServerConfig::Stdio {
            command: "test2".to_string(),
            args: vec![],
            env: HashMap::new(),
        };
        assert!(!mcp_cfg_eq(&cfg1, &cfg2));
    }

    #[test]
    fn test_mcp_cfg_eq_stdio_different_args() {
        let cfg1 = McpServerConfig::Stdio {
            command: "test".to_string(),
            args: vec!["arg1".to_string()],
            env: HashMap::new(),
        };
        let cfg2 = McpServerConfig::Stdio {
            command: "test".to_string(),
            args: vec!["arg2".to_string()],
            env: HashMap::new(),
        };
        assert!(!mcp_cfg_eq(&cfg1, &cfg2));
    }

    #[test]
    fn test_mcp_cfg_eq_sse_same() {
        let cfg1 = McpServerConfig::Sse {
            url: "http://test".to_string(),
            headers: HashMap::new(),
        };
        let cfg2 = McpServerConfig::Sse {
            url: "http://test".to_string(),
            headers: HashMap::new(),
        };
        assert!(mcp_cfg_eq(&cfg1, &cfg2));
    }

    #[test]
    fn test_mcp_cfg_eq_sse_different_url() {
        let cfg1 = McpServerConfig::Sse {
            url: "http://test1".to_string(),
            headers: HashMap::new(),
        };
        let cfg2 = McpServerConfig::Sse {
            url: "http://test2".to_string(),
            headers: HashMap::new(),
        };
        assert!(!mcp_cfg_eq(&cfg1, &cfg2));
    }

    #[test]
    fn test_mcp_cfg_eq_different_types() {
        let cfg1 = McpServerConfig::Stdio {
            command: "test".to_string(),
            args: vec![],
            env: HashMap::new(),
        };
        let cfg2 = McpServerConfig::Sse {
            url: "http://test".to_string(),
            headers: HashMap::new(),
        };
        assert!(!mcp_cfg_eq(&cfg1, &cfg2));
    }
}
