//! Linkkit JSON 格式解析器
//!
//! 提供比 XML 更简洁的调用格式，支持所有 Linkkit 标签的 JSON 表示。
//!
//! ## 模块结构
//!
//! - `doc` - 文档管理 (`doc`, `doc-ls`)
//! - `bash` - Bash 执行与管理
//! - `file` - 文件操作 (`ls`, `tree`, `grep`, `find`, `read`, `edit`, `write`)
//! - `tool` - 工具调用 (`use`, `tool-ls`, `tool-info`)
//! - `todo` - 待办事项管理
//! - `ask` - 用户询问
//! - `agent` - 子 Agent 调用
//! - `event` - 事件订阅
//! - `summary` - 总结
//!
//! ## 示例
//!
//! ```rust
//! use linkkit_parser::{LinkkitJson, LinkkitTag};
//!
//! // 文档阅读
//! let json = r#"{"doc": "bash", "line": "1-50"}"#;
//! let cmd = LinkkitJson::parse(json)?;
//! let tag = cmd.into_tag()?;
//!
//! // Bash 执行
//! let json = r#"{"bash": "ls -la", "backend": true}"#;
//! let cmd = LinkkitJson::parse(json)?;
//! let tag = cmd.into_tag()?;
//!
//! // 批量解析
//! let json = r#"[{"doc": "bash"}, {"bash": "ls"}]"#;
//! let cmds = LinkkitJson::parse_batch(json)?;
//! # Ok::<(), linkkit_parser::LinkkitError>(())
//! ```

mod doc;
mod bash;
mod file;
mod tool;
mod todo;
mod ask;
mod agent;
mod event;
mod summary;

pub use doc::*;
pub use bash::*;
pub use file::*;
pub use tool::*;
pub use todo::*;
pub use ask::*;
pub use agent::*;
pub use event::*;
pub use summary::*;

use serde::{Deserialize, Serialize};
use crate::error::{LinkkitError, LinkkitResult};
use crate::tags::LinkkitTag;

/// Linkkit JSON 命令格式
///
/// 使用 `#[serde(untagged)]` 实现自动类型识别，根据 JSON 键名匹配对应的命令类型。
///
/// **注意**：枚举变体顺序影响匹配优先级，越具体的类型应该放在前面。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LinkkitJson {
    // ─── 文档管理 ───────────────────────────────────────────────────────
    /// `{ "doc": "tool_name", "line": "1-50", "say": "..." }`
    DocRead(DocRead),

    // ─── Bash 执行 ──────────────────────────────────────────────────────
    /// `{ "bash": "command", "backend": true, "say": "..." }`
    Bash(BashCommand),

    // ─── 文件操作 ───────────────────────────────────────────────────────
    /// `{ "ls": "/path", "tail": "50", "say": "..." }`
    Ls(LsCommand),

    /// `{ "tree": "/path", "say": "..." }` 或 `{ "tree": { "dir": [...] } }`
    Tree(TreeCommand),

    /// `{ "grep": "/path", "pattern": "...", "say": "..." }`
    Grep(GrepCommand),

    /// `{ "find": "*.txt", "say": "..." }`
    Find(FindCommand),

    // ─── 工具调用 ───────────────────────────────────────────────────────
    /// `{ "use": "tool_name", "meta": {...}, "say": "..." }`
    ToolUse(ToolUse),

    // ─── TODO 管理 ──────────────────────────────────────────────────────
    /// `{ "todo": "markdown_content", "say": "..." }`
    Todo(TodoCommand),

    // ─── 询问用户 ───────────────────────────────────────────────────────
    /// `{ "ask": [...], "say": "..." }`
    Ask(AskCommand),

    // ─── 子 Agent ───────────────────────────────────────────────────────
    /// `{ "agent": "prompt", "backend": true, "say": "..." }`
    Agent(AgentCommand),

    // ─── 事件订阅 ───────────────────────────────────────────────────────
    /// `{ "event": "task_id", "arg": {...}, "say": "..." }`
    Event(EventCommand),

    // ─── 总结 ───────────────────────────────────────────────────────────
    /// `{ "summary": "content" }`
    Summary(SummaryCommand),
}

impl LinkkitJson {
    /// 解析单个 JSON 字符串为 LinkkitJson
    ///
    /// # 示例
    ///
    /// ```
    /// use linkkit_parser::LinkkitJson;
    ///
    /// let json = r#"{"doc": "bash", "line": "1-50"}"#;
    /// let cmd = LinkkitJson::parse(json)?;
    /// # Ok::<(), linkkit_parser::LinkkitError>(())
    /// ```
    pub fn parse(input: &str) -> LinkkitResult<Self> {
        serde_json::from_str(input).map_err(LinkkitError::Json)
    }

    /// 解析 JSON 数组，支持批量调用
    ///
    /// # 示例
    ///
    /// ```
    /// use linkkit_parser::LinkkitJson;
    ///
    /// let json = r#"[
    ///     {"doc": "bash"},
    ///     {"bash": "ls -la"}
    /// ]"#;
    /// let cmds = LinkkitJson::parse_batch(json)?;
    /// assert_eq!(cmds.len(), 2);
    /// # Ok::<(), linkkit_parser::LinkkitError>(())
    /// ```
    pub fn parse_batch(input: &str) -> LinkkitResult<Vec<Self>> {
        serde_json::from_str(input).map_err(LinkkitError::Json)
    }

    /// 转换为 LinkkitTag（统一处理）
    ///
    /// # 示例
    ///
    /// ```
    /// use linkkit_parser::{LinkkitJson, LinkkitTag};
    ///
    /// let json = r#"{"doc": "bash"}"#;
    /// let cmd = LinkkitJson::parse(json)?;
    /// let tag = cmd.into_tag()?;
    ///
    /// match tag {
    ///     LinkkitTag::DocRead { name, .. } => {
    ///         assert_eq!(name, Some("bash".to_string()));
    ///     }
    ///     _ => panic!("Expected DocRead tag"),
    /// }
    /// # Ok::<(), linkkit_parser::LinkkitError>(())
    /// ```
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        match self {
            Self::DocRead(cmd) => cmd.into_tag(),
            Self::Bash(cmd) => cmd.into_tag(),
            Self::Ls(cmd) => cmd.into_tag(),
            Self::Tree(cmd) => cmd.into_tag(),
            Self::Grep(cmd) => cmd.into_tag(),
            Self::Find(cmd) => cmd.into_tag(),
            Self::ToolUse(cmd) => cmd.into_tag(),
            Self::Todo(cmd) => cmd.into_tag(),
            Self::Ask(cmd) => cmd.into_tag(),
            Self::Agent(cmd) => cmd.into_tag(),
            Self::Event(cmd) => cmd.into_tag(),
            Self::Summary(cmd) => cmd.into_tag(),
        }
    }

    /// 批量转换为 LinkkitTag
    pub fn into_tags(cmds: Vec<Self>) -> LinkkitResult<Vec<LinkkitTag>> {
        cmds.into_iter().map(|cmd| cmd.into_tag()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single() {
        let json = r#"{"doc": "bash"}"#;
        let cmd = LinkkitJson::parse(json).unwrap();
        assert!(matches!(cmd, LinkkitJson::DocRead(_)));
    }

    #[test]
    fn test_parse_batch() {
        let json = r#"[
            {"doc": "bash"},
            {"bash": "ls -la"}
        ]"#;
        let cmds = LinkkitJson::parse_batch(json).unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(matches!(cmds[0], LinkkitJson::DocRead(_)));
        assert!(matches!(cmds[1], LinkkitJson::Bash(_)));
    }

    #[test]
    fn test_into_tag() {
        let json = r#"{"doc": "bash", "line": "1-50"}"#;
        let cmd = LinkkitJson::parse(json).unwrap();
        let tag = cmd.into_tag().unwrap();

        match tag {
            LinkkitTag::DocRead { name, line } => {
                assert_eq!(name, Some("bash".to_string()));
                assert_eq!(line, Some("1-50".to_string()));
            }
            _ => panic!("Expected DocRead tag"),
        }
    }

    #[test]
    fn test_into_tags_batch() {
        let json = r#"[
            {"doc": "bash"},
            {"bash": "ls"}
        ]"#;
        let cmds = LinkkitJson::parse_batch(json).unwrap();
        let tags = LinkkitJson::into_tags(cmds).unwrap();
        
        assert_eq!(tags.len(), 2);
        assert!(matches!(tags[0], LinkkitTag::DocRead { .. }));
        assert!(matches!(tags[1], LinkkitTag::Bash { .. }));
    }
}
