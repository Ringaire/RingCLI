use neko_core::tools::{BuiltinToolKind, HybridToolRegistry, Tool};
use std::sync::Arc;

use crate::builtin::BuiltinTool;

/// 初始化混合模式工具注册表。
///
/// 将 17 种内置工具以统一的 `BuiltinTool` 枚举形式注册到 `HybridToolRegistry`：
/// - **builtin 层**：不可变 HashMap，启动时填充，读无锁
/// - **dynamic 层**：`RwLock<HashMap>`，支持运行时注册（MCP 工具等）
///
/// # 注册方式
///
/// 每个内置工具装箱为 `Arc<dyn Tool>`（外层 trait object），但底层是
/// `BuiltinTool` 枚举变体，`execute` 内部走 `match` 静态分发到具体工具。
/// 受 `neko-tools → neko-core` 依赖方向约束，无法在 core 层完全消除虚表。
///
/// # 工具列表（17 种）
///
/// bash / read_file / write_file / edit_file / tree / glob / grep /
/// web_fetch / web_search / lsp_diagnostics / lsp_refs / list_sessions /
/// search_sessions / memory / todo / token_count / shell
pub fn init_hybrid_registry() -> HybridToolRegistry {
    let builtin_tools: Vec<(&'static str, Arc<dyn Tool>)> = BuiltinToolKind::all()
        .iter()
        .copied()
        .filter_map(|kind| {
            let tool = BuiltinTool::from_name(kind.name())?;
            Some((kind.name(), Arc::new(tool) as Arc<dyn Tool>))
        })
        .collect();

    HybridToolRegistry::new().with_builtin_tools(builtin_tools)
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use neko_core::tools::{BuiltinToolKind, ToolRegistry};

    #[test]
    fn test_init_hybrid_registry_all_tools_registered() {
        let registry = init_hybrid_registry();

        let expected_tools = [
            "bash", "read_file", "write_file", "edit_file", "tree",
            "glob", "grep", "web_fetch", "web_search",
            "lsp_diagnostics", "lsp_refs",
            "list_sessions", "search_sessions",
            "memory", "todo", "token_count", "shell",
        ];

        for tool_name in &expected_tools {
            let tool = registry.get(tool_name);
            assert!(tool.is_some(), "工具 '{}' 未注册到 HybridToolRegistry", tool_name);
            assert_eq!(tool.unwrap().name(), *tool_name, "工具名称不匹配: {}", tool_name);
        }

        let all_tools = registry.list();
        assert_eq!(all_tools.len(), 17, "应该注册 17 种工具，实际 {}", all_tools.len());
    }

    #[test]
    fn test_init_hybrid_registry_matches_builtin_tool_kind() {
        let registry = init_hybrid_registry();

        for kind in BuiltinToolKind::all() {
            let tool_name = kind.name();
            let tool = registry.get(tool_name);
            assert!(
                tool.is_some(),
                "BuiltinToolKind::{:?} 对应的工具 '{}' 未注册",
                kind, tool_name
            );
        }
    }

    #[test]
    fn test_init_hybrid_registry_tools_have_valid_metadata() {
        let registry = init_hybrid_registry();

        for tool in registry.list() {
            assert!(!tool.name().is_empty(), "工具名称不能为空");
            assert!(!tool.description().is_empty(), "工具 '{}' 描述不能为空", tool.name());
            assert!(
                tool.input_schema().is_object(),
                "工具 '{}' 的 input_schema 必须是 JSON object",
                tool.name()
            );
        }
    }

    #[test]
    fn test_hybrid_registry_supports_dynamic_registration() {
        let registry = init_hybrid_registry();

        use async_trait::async_trait;
        use neko_core::tools::{ToolContext, ToolResult};
        use serde_json::{json, Value};

        struct MockMcpTool;

        #[async_trait]
        impl Tool for MockMcpTool {
            fn name(&self) -> &str { "mock_mcp_tool" }
            fn description(&self) -> &str { "A mock MCP tool for testing" }
            fn input_schema(&self) -> Value { json!({ "type": "object" }) }
            async fn execute(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
                ToolResult::ok_text("mock result")
            }
        }

        registry.register_arc(Arc::new(MockMcpTool));

        assert_eq!(registry.list().len(), 18);
        assert!(registry.get("bash").is_some());
        assert!(registry.get("mock_mcp_tool").is_some());
    }
}
