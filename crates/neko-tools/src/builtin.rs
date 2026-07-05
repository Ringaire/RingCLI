//! 内置工具的统一枚举类型。
//!
//! # 设计动机
//!
//! 将 17 种内置工具统一为单一枚举类型，直接持有具体工具实例：
//! - **完整性约束**：新增工具时编译器强制更新所有 `match` 分支
//! - **统一管理**：便于未来添加横切关注点（统计、缓存、权限装饰等）
//! - **架构清晰**：替代此前散落的独立 struct 装箱
//!
//! # 架构约束说明
//!
//! 受依赖方向 `neko-tools → neko-core` 限制，`neko-core` 无法引用具体工具类型。
//! 因此注册时仍装箱为 `Arc<dyn Tool>`，外层为 trait object。
//! 本枚举的收益在架构统一与编译期完整性，而非完全消除虚表分发
//! （那需泛型化整个装饰器链，侵入式且破坏分层）。

use async_trait::async_trait;
use neko_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::Value;

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

/// 17 种内置工具的统一枚举。
///
/// 所有变体持有 unit struct 工具（无运行时状态），构造零成本。
/// 通过 `match` 分发到具体工具方法，编译器可内联优化。
pub enum BuiltinTool {
    Bash(BashTool),
    Read(ReadFileTool),
    Write(WriteFileTool),
    Edit(EditFileTool),
    Tree(TreeTool),
    Glob(GlobTool),
    Grep(GrepTool),
    WebFetch(WebFetchTool),
    WebSearch(WebSearchTool),
    LspDiagnostics(LspDiagnosticsTool),
    LspRefs(LspRefsTool),
    ListSessions(ListSessionsTool),
    SearchSessions(SearchSessionsTool),
    Memory(MemoryTool),
    Todo(TodoTool),
    TokenCount(TokenCountTool),
    Shell(ShellTool),
}

impl BuiltinTool {
    /// 按名称构造对应的内置工具实例。
    ///
    /// 用于注册表初始化：根据工具名（来自 `BuiltinToolKind::name()`）
    /// 构造枚举变体。unit struct 构造零成本。
    /// 未匹配返回 `None`（动态工具/MCP 工具不在此列）。
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "bash"            => Self::Bash(BashTool),
            "read_file"       => Self::Read(ReadFileTool),
            "write_file"      => Self::Write(WriteFileTool),
            "edit_file"       => Self::Edit(EditFileTool),
            "tree"            => Self::Tree(TreeTool),
            "glob"            => Self::Glob(GlobTool),
            "grep"            => Self::Grep(GrepTool),
            "web_fetch"       => Self::WebFetch(WebFetchTool),
            "web_search"      => Self::WebSearch(WebSearchTool),
            "lsp_diagnostics" => Self::LspDiagnostics(LspDiagnosticsTool),
            "lsp_refs"        => Self::LspRefs(LspRefsTool),
            "list_sessions"   => Self::ListSessions(ListSessionsTool),
            "search_sessions" => Self::SearchSessions(SearchSessionsTool),
            "memory"          => Self::Memory(MemoryTool),
            "todo"            => Self::Todo(TodoTool),
            "token_count"     => Self::TokenCount(TokenCountTool),
            "shell"           => Self::Shell(ShellTool),
            _ => return None,
        })
    }
}

#[async_trait]
impl Tool for BuiltinTool {
    fn name(&self) -> &str {
        match self {
            Self::Bash(t)           => t.name(),
            Self::Read(t)           => t.name(),
            Self::Write(t)          => t.name(),
            Self::Edit(t)           => t.name(),
            Self::Tree(t)           => t.name(),
            Self::Glob(t)           => t.name(),
            Self::Grep(t)           => t.name(),
            Self::WebFetch(t)       => t.name(),
            Self::WebSearch(t)      => t.name(),
            Self::LspDiagnostics(t) => t.name(),
            Self::LspRefs(t)        => t.name(),
            Self::ListSessions(t)   => t.name(),
            Self::SearchSessions(t) => t.name(),
            Self::Memory(t)         => t.name(),
            Self::Todo(t)           => t.name(),
            Self::TokenCount(t)     => t.name(),
            Self::Shell(t)          => t.name(),
        }
    }

    fn description(&self) -> &str {
        match self {
            Self::Bash(t)           => t.description(),
            Self::Read(t)           => t.description(),
            Self::Write(t)          => t.description(),
            Self::Edit(t)           => t.description(),
            Self::Tree(t)           => t.description(),
            Self::Glob(t)           => t.description(),
            Self::Grep(t)           => t.description(),
            Self::WebFetch(t)       => t.description(),
            Self::WebSearch(t)      => t.description(),
            Self::LspDiagnostics(t) => t.description(),
            Self::LspRefs(t)        => t.description(),
            Self::ListSessions(t)   => t.description(),
            Self::SearchSessions(t) => t.description(),
            Self::Memory(t)         => t.description(),
            Self::Todo(t)           => t.description(),
            Self::TokenCount(t)     => t.description(),
            Self::Shell(t)          => t.description(),
        }
    }

    fn input_schema(&self) -> Value {
        match self {
            Self::Bash(t)           => t.input_schema(),
            Self::Read(t)           => t.input_schema(),
            Self::Write(t)          => t.input_schema(),
            Self::Edit(t)           => t.input_schema(),
            Self::Tree(t)           => t.input_schema(),
            Self::Glob(t)           => t.input_schema(),
            Self::Grep(t)           => t.input_schema(),
            Self::WebFetch(t)       => t.input_schema(),
            Self::WebSearch(t)      => t.input_schema(),
            Self::LspDiagnostics(t) => t.input_schema(),
            Self::LspRefs(t)        => t.input_schema(),
            Self::ListSessions(t)   => t.input_schema(),
            Self::SearchSessions(t) => t.input_schema(),
            Self::Memory(t)         => t.input_schema(),
            Self::Todo(t)           => t.input_schema(),
            Self::TokenCount(t)     => t.input_schema(),
            Self::Shell(t)          => t.input_schema(),
        }
    }

    fn prompt(&self) -> Option<&str> {
        match self {
            Self::Bash(t)           => t.prompt(),
            Self::Read(t)           => t.prompt(),
            Self::Write(t)          => t.prompt(),
            Self::Edit(t)           => t.prompt(),
            Self::Tree(t)           => t.prompt(),
            Self::Glob(t)           => t.prompt(),
            Self::Grep(t)           => t.prompt(),
            Self::WebFetch(t)       => t.prompt(),
            Self::WebSearch(t)      => t.prompt(),
            Self::LspDiagnostics(t) => t.prompt(),
            Self::LspRefs(t)        => t.prompt(),
            Self::ListSessions(t)   => t.prompt(),
            Self::SearchSessions(t) => t.prompt(),
            Self::Memory(t)         => t.prompt(),
            Self::Todo(t)           => t.prompt(),
            Self::TokenCount(t)     => t.prompt(),
            Self::Shell(t)          => t.prompt(),
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        match self {
            Self::Bash(t)           => t.execute(input, ctx).await,
            Self::Read(t)           => t.execute(input, ctx).await,
            Self::Write(t)          => t.execute(input, ctx).await,
            Self::Edit(t)           => t.execute(input, ctx).await,
            Self::Tree(t)           => t.execute(input, ctx).await,
            Self::Glob(t)           => t.execute(input, ctx).await,
            Self::Grep(t)           => t.execute(input, ctx).await,
            Self::WebFetch(t)       => t.execute(input, ctx).await,
            Self::WebSearch(t)      => t.execute(input, ctx).await,
            Self::LspDiagnostics(t) => t.execute(input, ctx).await,
            Self::LspRefs(t)        => t.execute(input, ctx).await,
            Self::ListSessions(t)   => t.execute(input, ctx).await,
            Self::SearchSessions(t) => t.execute(input, ctx).await,
            Self::Memory(t)         => t.execute(input, ctx).await,
            Self::Todo(t)           => t.execute(input, ctx).await,
            Self::TokenCount(t)     => t.execute(input, ctx).await,
            Self::Shell(t)          => t.execute(input, ctx).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neko_core::tools::BuiltinToolKind;

    #[test]
    fn test_builtin_tool_from_name_covers_all_kinds() {
        for kind in BuiltinToolKind::all() {
            let tool = BuiltinTool::from_name(kind.name());
            assert!(tool.is_some(), "BuiltinToolKind::{:?} ({}) 无对应 BuiltinTool", kind, kind.name());
            assert_eq!(tool.unwrap().name(), kind.name());
        }
    }

    #[test]
    fn test_builtin_tool_from_name_unknown_returns_none() {
        assert!(BuiltinTool::from_name("nonexistent").is_none());
        assert!(BuiltinTool::from_name("").is_none());
    }

    #[test]
    fn test_builtin_tool_names_unique() {
        let names: Vec<String> = BuiltinToolKind::all()
            .iter()
            .filter_map(|k| BuiltinTool::from_name(k.name()).map(|t| t.name().to_string()))
            .collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "内置工具名称必须唯一");
    }

    #[test]
    fn test_builtin_tool_metadata_nonempty() {
        for kind in BuiltinToolKind::all() {
            let tool = BuiltinTool::from_name(kind.name()).expect("from_name failed");
            assert!(!tool.name().is_empty(), "工具名称不能为空");
            assert!(!tool.description().is_empty(), "工具 {} 描述不能为空", tool.name());
            assert!(
                tool.input_schema().is_object(),
                "工具 {} schema 必须是 object", tool.name()
            );
        }
    }
}
