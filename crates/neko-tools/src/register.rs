use neko_core::tools::{HybridToolRegistry, Tool, ToolRegistry, ToolRegistryExt};
use std::sync::Arc;

use crate::tools::{
    bash::BashTool,
    edit_file::EditFileTool,
    glob::GlobTool,
    grep::GrepTool,
    lsp_diagnostics::LspDiagnosticsTool,
    lsp_refs::LspRefsTool,
    memory::MemoryTool,
    read_file::ReadFileTool,
    search_sessions::SearchSessionsTool,
    sessions::ListSessionsTool,
    shell::ShellTool,
    todo::TodoTool,
    token_count::TokenCountTool,
    tree::TreeTool,
    web_fetch::WebFetchTool,
    web_search::WebSearchTool,
    write_file::WriteFileTool,
};

/// 注册所有内置工具到传统的 ToolRegistry（向后兼容）。
///
/// 用于 `DefaultToolRegistry` 等基于 trait object 的注册表。
pub fn register_all(registry: &dyn ToolRegistry) {
    registry.register(BashTool);
    registry.register(ReadFileTool);
    registry.register(WriteFileTool);
    registry.register(EditFileTool);
    registry.register(TreeTool);
    registry.register(GlobTool);
    registry.register(GrepTool);
    registry.register(WebFetchTool);
    registry.register(WebSearchTool);
    registry.register(LspDiagnosticsTool);
    registry.register(LspRefsTool);
    registry.register(MemoryTool);
    registry.register(TodoTool);
    registry.register(TokenCountTool);
    registry.register(ListSessionsTool);
    registry.register(SearchSessionsTool);
    registry.register(ShellTool);
}

/// 初始化混合模式工具注册表（推荐方式）。
///
/// 将 17 种内置工具注册到 `HybridToolRegistry`，支持：
/// - 内置工具的静态分发（零虚表查找）
/// - 动态工具的运行时注册（如 MCP 工具）
///
/// # 性能优势
///
/// 相比 `register_all` + `DefaultToolRegistry`：
/// - 内置工具查找无锁竞争（O(1) HashMap 查找）
/// - 支持静态分发优化（编译器可内联）
/// - 动态工具使用 RwLock，读多写少场景性能优秀
///
/// # 使用示例
///
/// ```rust,ignore
/// use neko_tools::init_hybrid_registry;
///
/// let registry = init_hybrid_registry();
/// let bash_tool = registry.get("bash").unwrap();
/// ```
///
/// # 工具列表
///
/// 注册以下 17 种内置工具：
/// 1. bash - Bash 命令执行
/// 2. read_file - 文件读取
/// 3. write_file - 文件写入
/// 4. edit_file - 文件编辑（精确替换）
/// 5. tree - 文件树展示
/// 6. glob - Glob 模式匹配
/// 7. grep - 内容搜索
/// 8. web_fetch - Web 资源获取
/// 9. web_search - Web 搜索
/// 10. lsp_diagnostics - LSP 诊断信息
/// 11. lsp_refs - LSP 引用查找
/// 12. list_sessions - 会话列表
/// 13. search_sessions - 会话搜索
/// 14. memory - 记忆管理
/// 15. todo - 待办事项管理
/// 16. token_count - Token 计数
/// 17. shell - Shell 工具
pub fn init_hybrid_registry() -> HybridToolRegistry {
    let builtin_tools: Vec<(&'static str, Arc<dyn Tool>)> = vec![
        // ── 文件系统工具 ──────────────────────────────────────────────────────────
        ("bash", Arc::new(BashTool) as Arc<dyn Tool>),
        ("read_file", Arc::new(ReadFileTool)),
        ("write_file", Arc::new(WriteFileTool)),
        ("edit_file", Arc::new(EditFileTool)),
        ("tree", Arc::new(TreeTool)),
        // ── 搜索工具 ──────────────────────────────────────────────────────────────
        ("glob", Arc::new(GlobTool)),
        ("grep", Arc::new(GrepTool)),
        // ── 网络工具 ──────────────────────────────────────────────────────────────
        ("web_fetch", Arc::new(WebFetchTool)),
        ("web_search", Arc::new(WebSearchTool)),
        // ── LSP 工具 ──────────────────────────────────────────────────────────────
        ("lsp_diagnostics", Arc::new(LspDiagnosticsTool)),
        ("lsp_refs", Arc::new(LspRefsTool)),
        // ── 会话管理工具 ──────────────────────────────────────────────────────────
        ("list_sessions", Arc::new(ListSessionsTool)),
        ("search_sessions", Arc::new(SearchSessionsTool)),
        // ── Agent 辅助工具 ────────────────────────────────────────────────────────
        ("memory", Arc::new(MemoryTool)),
        ("todo", Arc::new(TodoTool)),
        ("token_count", Arc::new(TokenCountTool)),
        ("shell", Arc::new(ShellTool)),
    ];

    HybridToolRegistry::new().with_builtin_tools(builtin_tools)
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use neko_core::tools::BuiltinToolKind;

    #[test]
    fn test_init_hybrid_registry_all_tools_registered() {
        let registry = init_hybrid_registry();

        // 验证所有 17 种工具都已注册
        let expected_tools = [
            "bash",
            "read_file",
            "write_file",
            "edit_file",
            "tree",
            "glob",
            "grep",
            "web_fetch",
            "web_search",
            "lsp_diagnostics",
            "lsp_refs",
            "list_sessions",
            "search_sessions",
            "memory",
            "todo",
            "token_count",
            "shell",
        ];

        for tool_name in &expected_tools {
            let tool = registry.get(tool_name);
            assert!(
                tool.is_some(),
                "工具 '{}' 未注册到 HybridToolRegistry",
                tool_name
            );
            assert_eq!(
                tool.unwrap().name(),
                *tool_name,
                "工具名称不匹配: {}",
                tool_name
            );
        }

        // 验证注册的工具数量正确
        let all_tools = registry.list();
        assert_eq!(
            all_tools.len(),
            17,
            "应该注册 17 种工具，实际注册了 {} 种",
            all_tools.len()
        );
    }

    #[test]
    fn test_init_hybrid_registry_matches_builtin_tool_kind() {
        let registry = init_hybrid_registry();

        // 验证注册的工具名称与 BuiltinToolKind 枚举一致
        for kind in BuiltinToolKind::all() {
            let tool_name = kind.name();
            let tool = registry.get(tool_name);
            assert!(
                tool.is_some(),
                "BuiltinToolKind::{:?} 对应的工具 '{}' 未注册",
                kind,
                tool_name
            );
        }
    }

    #[test]
    fn test_init_hybrid_registry_tools_have_valid_metadata() {
        let registry = init_hybrid_registry();

        for tool in registry.list() {
            // 验证工具名称非空
            assert!(
                !tool.name().is_empty(),
                "工具名称不能为空: {:?}",
                tool.name()
            );

            // 验证工具描述非空
            assert!(
                !tool.description().is_empty(),
                "工具 '{}' 的描述不能为空",
                tool.name()
            );

            // 验证工具有有效的 input_schema
            let schema = tool.input_schema();
            assert!(
                schema.is_object(),
                "工具 '{}' 的 input_schema 必须是 JSON object",
                tool.name()
            );
        }
    }

    #[test]
    fn test_hybrid_registry_supports_dynamic_registration() {
        let registry = init_hybrid_registry();

        // 验证可以动态注册额外工具（模拟 MCP 工具）
        use async_trait::async_trait;
        use neko_core::tools::{ToolContext, ToolResult};
        use serde_json::{json, Value};

        struct MockMcpTool;

        #[async_trait]
        impl Tool for MockMcpTool {
            fn name(&self) -> &str {
                "mock_mcp_tool"
            }

            fn description(&self) -> &str {
                "A mock MCP tool for testing"
            }

            fn input_schema(&self) -> Value {
                json!({ "type": "object" })
            }

            async fn execute(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
                ToolResult::ok_text("mock result")
            }
        }

        // 动态注册 MCP 工具
        registry.register_arc(Arc::new(MockMcpTool));

        // 验证内置工具 + 动态工具共存
        assert_eq!(registry.list().len(), 18); // 17 内置 + 1 动态
        assert!(registry.get("bash").is_some()); // 内置工具仍可访问
        assert!(registry.get("mock_mcp_tool").is_some()); // 动态工具可访问
    }
}
