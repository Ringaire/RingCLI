// ── BuiltinTool 枚举：内置工具的静态分发层 ────────────────────────────────

//! 内置工具枚举定义，为静态分发提供类型基础。
//!
//! # 架构说明
//!
//! 由于 `neko-core` 和 `neko-tools` 的依赖关系（`neko-tools` → `neko-core`），
//! 不能在 `neko-core` 中直接引用具体工具类型。因此采用两层设计：
//!
//! - **Layer 1 (neko-core)**: 定义 `BuiltinToolKind` 类型标识枚举
//! - **Layer 2 (neko-tools)**: 实现具体的 `BuiltinTool` 包装枚举
//!
//! # 性能优势
//!
//! 使用枚举静态分发替代 `Arc<dyn Tool>` 的动态分发：
//! - **零虚表查找**: `match` 分支在编译期确定
//! - **内联友好**: 编译器可以内联小型工具方法
//! - **缓存友好**: 避免间接跳转，提高指令缓存命中率
//!
//! # 使用示例
//!
//! ```rust,ignore
//! // 在 neko-tools 中实现：
//! pub enum BuiltinTool {
//!     Bash(BashTool),
//!     Read(ReadFileTool),
//!     // ...
//! }
//!
//! #[async_trait]
//! impl Tool for BuiltinTool {
//!     async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
//!         match self {
//!             Self::Bash(t) => t.execute(input, ctx).await,  // 静态分发
//!             Self::Read(t) => t.execute(input, ctx).await,
//!             // ...
//!         }
//!     }
//! }
//! ```

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use super::{Tool, ToolContext, ToolResult};

/// 内置工具类型标识枚举。
///
/// 定义所有内置工具的类型标识，用于：
/// - 工具发现和枚举
/// - 类型安全的工具查找
/// - 静态分发的类型基础
///
/// # 设计原则
///
/// - **完整性**: 包含所有内置工具，与 `neko-tools` 中的实现一一对应
/// - **轻量级**: 仅携带类型信息，无运行时状态
/// - **可扩展**: 新增工具时，编译器会强制更新所有 match 分支
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinToolKind {
    // ── 文件系统工具 ──────────────────────────────────────────────────────────

    /// Bash 命令执行工具
    Bash,

    /// 文件读取工具
    Read,

    /// 文件写入工具
    Write,

    /// 文件编辑工具（精确替换）
    Edit,

    /// 文件树展示工具
    Tree,

    // ── 搜索工具 ──────────────────────────────────────────────────────────────

    /// Glob 模式匹配工具
    Glob,

    /// 内容搜索工具（Grep）
    Grep,

    // ── 网络工具 ──────────────────────────────────────────────────────────────

    /// Web 资源获取工具
    WebFetch,

    /// Web 搜索工具
    WebSearch,

    // ── LSP 工具 ──────────────────────────────────────────────────────────────

    /// LSP 诊断信息工具
    LspDiagnostics,

    /// LSP 引用查找工具
    LspRefs,

    // ── 会话管理工具 ──────────────────────────────────────────────────────────

    /// 会话列表工具
    Sessions,

    /// 会话搜索工具
    SearchSessions,

    // ── Agent 辅助工具 ────────────────────────────────────────────────────────

    /// 记忆管理工具
    Memory,

    /// 待办事项管理工具
    Todo,

    /// Token 计数工具
    TokenCount,

    /// Shell 工具
    Shell,
}

impl BuiltinToolKind {
    /// 获取工具名称（与 Tool trait 的 `name()` 方法保持一致）。
    ///
    /// 使用 `const fn` 保证零运行时开销。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Read => "read_file",
            Self::Write => "write_file",
            Self::Edit => "edit_file",
            Self::Tree => "tree",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::WebFetch => "web_fetch",
            Self::WebSearch => "web_search",
            Self::LspDiagnostics => "lsp_diagnostics",
            Self::LspRefs => "lsp_refs",
            Self::Sessions => "list_sessions",
            Self::SearchSessions => "search_sessions",
            Self::Memory => "memory",
            Self::Todo => "todo",
            Self::TokenCount => "token_count",
            Self::Shell => "shell",
        }
    }

    /// 列出所有内置工具类型。
    ///
    /// 用于工具发现和遍历。
    pub const fn all() -> &'static [Self] {
        &[
            Self::Bash,
            Self::Read,
            Self::Write,
            Self::Edit,
            Self::Tree,
            Self::Glob,
            Self::Grep,
            Self::WebFetch,
            Self::WebSearch,
            Self::LspDiagnostics,
            Self::LspRefs,
            Self::Sessions,
            Self::SearchSessions,
            Self::Memory,
            Self::Todo,
            Self::TokenCount,
            Self::Shell,
        ]
    }

    /// 根据工具名称查找对应的 `BuiltinToolKind`。
    ///
    /// # 性能
    ///
    /// O(n) 线性查找，但由于工具数量少（~17 个），性能可接受。
    /// 如果性能成为瓶颈，可以使用 `phf` crate 实现编译期哈希表。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|kind| kind.name() == name)
    }

    /// 获取内置工具的总数。
    pub const fn count() -> usize {
        17
    }
}

// ── BuiltinTool 占位符实现 ────────────────────────────────────────────────────

/// 内置工具占位符。
///
/// 在 `neko-core` 中提供占位实现，具体工具包装在 `neko-tools` 中实现。
/// 此类型用于类型系统和接口定义，不直接使用。
///
/// # 实现位置
///
/// 实际的 `BuiltinTool` 枚举应在 `neko-tools` crate 中实现：
///
/// ```rust,ignore
/// // crates/neko-tools/src/builtin.rs
/// pub enum BuiltinTool {
///     Bash(BashTool),
///     Read(ReadFileTool),
///     Write(WriteFileTool),
///     Edit(EditFileTool),
///     Tree(TreeTool),
///     Glob(GlobTool),
///     Grep(GrepTool),
///     WebFetch(WebFetchTool),
///     WebSearch(WebSearchTool),
///     LspDiagnostics(LspDiagnosticsTool),
///     LspRefs(LspRefsTool),
///     Sessions(ListSessionsTool),
///     SearchSessions(SearchSessionsTool),
///     Memory(MemoryTool),
///     Todo(TodoTool),
///     TokenCount(TokenCountTool),
///     Shell(ShellTool),
/// }
///
/// #[async_trait]
/// impl Tool for BuiltinTool {
///     fn name(&self) -> &str {
///         match self {
///             Self::Bash(t) => t.name(),
///             Self::Read(t) => t.name(),
///             // ... 其他工具
///         }
///     }
///
///     fn description(&self) -> &str {
///         match self {
///             Self::Bash(t) => t.description(),
///             Self::Read(t) => t.description(),
///             // ... 其他工具
///         }
///     }
///
///     fn input_schema(&self) -> Value {
///         match self {
///             Self::Bash(t) => t.input_schema(),
///             Self::Read(t) => t.input_schema(),
///             // ... 其他工具
///         }
///     }
///
///     fn prompt(&self) -> Option<&str> {
///         match self {
///             Self::Bash(t) => t.prompt(),
///             Self::Read(t) => t.prompt(),
///             // ... 其他工具
///         }
///     }
///
///     async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
///         // 关键优化点：静态分发，编译器可以内联优化每个分支
///         match self {
///             Self::Bash(t) => t.execute(input, ctx).await,
///             Self::Read(t) => t.execute(input, ctx).await,
///             // ... 其他工具
///         }
///     }
/// }
/// ```
pub struct BuiltinTool {
    kind: BuiltinToolKind,
    inner: Arc<dyn Tool>,
}

impl std::fmt::Debug for BuiltinTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltinTool")
            .field("kind", &self.kind)
            .field("name", &self.name())
            .finish()
    }
}

impl BuiltinTool {
    /// 创建内置工具包装。
    ///
    /// # 注意
    ///
    /// 这是占位符实现。在实际使用中，应该在 `neko-tools` 中创建
    /// 真正的枚举包装，直接持有具体工具实例（而非 trait object）。
    pub fn new(kind: BuiltinToolKind, tool: Arc<dyn Tool>) -> Self {
        Self { kind, inner: tool }
    }

    /// 获取工具类型标识。
    pub fn kind(&self) -> BuiltinToolKind {
        self.kind
    }

    /// 获取底层工具（用于兼容现有 API）。
    pub fn as_tool(&self) -> &Arc<dyn Tool> {
        &self.inner
    }
}

#[async_trait]
impl Tool for BuiltinTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn input_schema(&self) -> Value {
        self.inner.input_schema()
    }

    fn prompt(&self) -> Option<&str> {
        self.inner.prompt()
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        self.inner.execute(input, ctx).await
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_tool_kind_name() {
        assert_eq!(BuiltinToolKind::Bash.name(), "bash");
        assert_eq!(BuiltinToolKind::Read.name(), "read_file");
        assert_eq!(BuiltinToolKind::Write.name(), "write_file");
        assert_eq!(BuiltinToolKind::Edit.name(), "edit_file");
    }

    #[test]
    fn test_builtin_tool_kind_from_name() {
        assert_eq!(
            BuiltinToolKind::from_name("bash"),
            Some(BuiltinToolKind::Bash)
        );
        assert_eq!(
            BuiltinToolKind::from_name("read_file"),
            Some(BuiltinToolKind::Read)
        );
        assert_eq!(BuiltinToolKind::from_name("nonexistent"), None);
    }

    #[test]
    fn test_builtin_tool_kind_all() {
        let all = BuiltinToolKind::all();
        assert_eq!(all.len(), 17);
        assert_eq!(BuiltinToolKind::count(), 17);

        // 验证所有工具名称唯一
        let names: Vec<&str> = all.iter().map(|k| k.name()).collect();
        let mut unique_names = names.clone();
        unique_names.sort_unstable();
        unique_names.dedup();
        assert_eq!(names.len(), unique_names.len(), "工具名称必须唯一");
    }

    #[test]
    fn test_builtin_tool_kind_round_trip() {
        // 验证所有工具类型都能通过名称往返转换
        for kind in BuiltinToolKind::all() {
            let name = kind.name();
            let recovered = BuiltinToolKind::from_name(name);
            assert_eq!(recovered, Some(*kind), "工具 {} 无法通过名称恢复", name);
        }
    }
}
