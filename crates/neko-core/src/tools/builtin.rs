// ── BuiltinToolKind：内置工具类型标识 ──────────────────────────────────────

//! 内置工具的类型标识枚举。
//!
//! # 设计说明
//!
//! `BuiltinToolKind` 仅携带类型信息（无运行时状态），用于：
//! - 工具发现与枚举（`all()` / `from_name()`）
//! - 注册表初始化时提供 `&'static str` 工具名（`name()` 是 const fn）
//! - 类型安全的工具查找键
//!
//! # 与 neko-tools::BuiltinTool 的关系
//!
//! 由于依赖方向 `neko-tools → neko-core`，`neko-core` 无法持有具体工具类型。
//! 因此：
//! - **本 crate（neko-core）**：定义 `BuiltinToolKind` 类型标识
//! - **neko-tools crate**：定义 `BuiltinTool` 枚举，持有具体工具实例（unit struct），
//!   通过 `BuiltinTool::from_name(kind.name())` 与本枚举建立映射
//!
//! 实际的工具分发枚举在 `neko_tools::builtin::BuiltinTool`。

use serde_json::Value;

/// 内置工具类型标识枚举。
///
/// 包含所有内置工具的类型标识。新增工具时：
/// 1. 在此枚举添加变体
/// 2. 在 `name()` 添加映射
/// 3. 在 `neko_tools::BuiltinTool` 添加对应变体
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinToolKind {
    // ── 文件系统工具 ──
    Bash,
    Read,
    Write,
    Edit,
    Tree,

    // ── 搜索工具 ──
    Glob,
    Grep,

    // ── 网络工具 ──
    WebFetch,
    WebSearch,

    // ── LSP 工具 ──
    LspDiagnostics,
    LspRefs,

    // ── 会话管理工具 ──
    Sessions,
    SearchSessions,

    // ── Agent 辅助工具 ──
    Memory,
    Todo,
    TokenCount,
    Shell,
}

impl BuiltinToolKind {
    /// 获取工具名称（与 `Tool` trait 的 `name()` 保持一致）。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bash            => "bash",
            Self::Read            => "read_file",
            Self::Write           => "write_file",
            Self::Edit            => "edit_file",
            Self::Tree            => "tree",
            Self::Glob            => "glob",
            Self::Grep            => "grep",
            Self::WebFetch        => "web_fetch",
            Self::WebSearch       => "web_search",
            Self::LspDiagnostics  => "lsp_diagnostics",
            Self::LspRefs         => "lsp_refs",
            Self::Sessions        => "list_sessions",
            Self::SearchSessions  => "search_sessions",
            Self::Memory          => "memory",
            Self::Todo            => "todo",
            Self::TokenCount      => "token_count",
            Self::Shell           => "shell",
        }
    }

    /// 列出所有内置工具类型。
    pub const fn all() -> &'static [Self] {
        &[
            Self::Bash, Self::Read, Self::Write, Self::Edit, Self::Tree,
            Self::Glob, Self::Grep,
            Self::WebFetch, Self::WebSearch,
            Self::LspDiagnostics, Self::LspRefs,
            Self::Sessions, Self::SearchSessions,
            Self::Memory, Self::Todo, Self::TokenCount, Self::Shell,
        ]
    }

    /// 根据工具名查找对应的 `BuiltinToolKind`。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|kind| kind.name() == name)
    }

    /// 获取内置工具总数。
    pub const fn count() -> usize {
        17
    }

    /// 占位：未来用于工具元数据查询。
    ///
    /// 当前返回空 `Value`，具体 schema 由 `neko_tools::BuiltinTool::input_schema` 提供。
    #[deprecated(note = "use neko_tools::BuiltinTool::input_schema via registry lookup")]
    pub fn input_schema(self) -> Value {
        Value::Object(Default::default())
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
        assert_eq!(BuiltinToolKind::from_name("bash"), Some(BuiltinToolKind::Bash));
        assert_eq!(BuiltinToolKind::from_name("read_file"), Some(BuiltinToolKind::Read));
        assert_eq!(BuiltinToolKind::from_name("nonexistent"), None);
    }

    #[test]
    fn test_builtin_tool_kind_all() {
        let all = BuiltinToolKind::all();
        assert_eq!(all.len(), 17);
        assert_eq!(BuiltinToolKind::count(), 17);

        let names: Vec<&str> = all.iter().map(|k| k.name()).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "工具名称必须唯一");
    }

    #[test]
    fn test_builtin_tool_kind_round_trip() {
        for kind in BuiltinToolKind::all() {
            let name = kind.name();
            assert_eq!(
                BuiltinToolKind::from_name(name),
                Some(*kind),
                "工具 {} 无法通过名称恢复", name
            );
        }
    }
}
