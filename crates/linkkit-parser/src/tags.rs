use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Linkkit 标签定义
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LinkkitTag {
    // ─── 文档管理 ───────────────────────────────────────────────────────
    /// `<doc-ls/>`
    DocLs,

    /// `<doc-read name="x"/>` 或 `<doc-read name="x" line="20-60"/>`
    DocRead {
        name: Option<String>,
        line: Option<String>,
    },

    // ─── 工具管理 ───────────────────────────────────────────────────────
    /// `<tool-ls/>` 或 `<tool-ls profile="true"/>`
    ToolLs { profile: bool },

    /// `<tool-info name="x"/>`
    ToolInfo { name: String },

    /// `<tool-use name="x">...</tool-use>`
    ToolUse { name: String, args: ToolArgs },

    /// `<tool-reload/>`
    ToolReload,

    // ─── 命令执行 ───────────────────────────────────────────────────────
    /// `<bash>command</bash>` 或带属性版本
    Bash {
        command: String,
        timeout: Option<u64>, // 秒
        tail: Option<usize>,
        bg: bool,
        at: Option<PathBuf>,
    },

    /// `<bash-ls/>` 或 `<bash-ls all="true"/>`
    BashLs { all: bool, find: Option<String> },

    /// `<bash-kill>TaskID</bash-kill>`
    BashKill { task_id: String },

    /// `<bash-log>TaskID</bash-log>` 或带行号/tail
    BashLog {
        task_id: String,
        line: Option<String>,
        tail: Option<usize>,
    },

    // ─── 文件操作 ───────────────────────────────────────────────────────
    /// `<read file="x"/>` 或带 line/tail
    Read {
        file: PathBuf,
        line: Option<String>,
        tail: Option<usize>,
    },

    /// `<edit file="x">content</edit>` (整文件覆盖)
    /// 或 `<edit file="x"><old>...</old><new>...</new></edit>` (局部替换)
    Edit {
        file: PathBuf,
        old: Option<String>,
        new: Option<String>,
        all: bool,
        content: Option<String>, // 整文件覆盖时使用
    },

    /// `<write file="x">content</write>` (与 Edit 整文件覆盖等价，但更明确)
    Write { file: PathBuf, content: String },

    // ─── 目录浏览 ───────────────────────────────────────────────────────
    /// `<tree path="src/"/>`
    Tree {
        path: PathBuf,
        level: Option<usize>,
        exclude: Option<String>,
        all: bool,
    },

    // ─── 网页抓取 ───────────────────────────────────────────────────────
    /// `<web-fetch>https://...</web-fetch>`
    WebFetch { url: String },

    // ─── TODO 管理 ──────────────────────────────────────────────────────
    /// `<todo-update>markdown</todo-update>`
    TodoUpdate { content: String },

    /// `<todo-done/>`
    TodoDone,

    /// `<todo-clear/>`
    TodoClear,

    // ─── 子 Agent ───────────────────────────────────────────────────────
    /// `<sub-agent>prompt</sub-agent>`
    SubAgent {
        prompt: String,
        name: Option<String>,
        mode: Option<String>,
    },

    /// `<sub-task/>` 或 `<sub-task all="true"/>`
    SubTask { all: bool, find: Option<String> },

    /// `<sub-cancel>SubTaskID</sub-cancel>`
    SubCancel { task_id: String },

    // ─── 事件订阅 ───────────────────────────────────────────────────────
    /// `<event form="..." .../>`
    Event {
        form: String,
        name: Option<String>,
        task: Option<String>,
        pid: Option<u32>,
        time: Option<String>,
        clock: Option<String>,
        day: Option<String>,
        everytime: Option<String>,
        path: Option<PathBuf>,
        shell: Option<String>,
        max: Option<usize>,
    },

    /// `<event-ls/>` 或 `<event-ls all="true"/>`
    EventLs { all: bool, find: Option<String> },

    /// `<event-cancel>EventID</event-cancel>` 或 `<event-cancel form="time"/>`
    EventCancel {
        id: Option<String>,
        form: Option<String>,
    },

    // ─── 询问用户 ───────────────────────────────────────────────────────
    /// `<ask>question</ask>` 或 `<ask options="A,B,C">question</ask>`
    Ask {
        question: String,
        options: Option<String>,
    },
}

/// 工具参数：单参数或多参数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolArgs {
    /// 单参数：直接文本内容
    Single(String),
    /// 多参数：命名参数映射
    Multiple(HashMap<String, String>),
}

impl LinkkitTag {
    /// 获取标签名称（用于错误提示和日志）
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::DocLs => "doc-ls",
            Self::DocRead { .. } => "doc-read",
            Self::ToolLs { .. } => "tool-ls",
            Self::ToolInfo { .. } => "tool-info",
            Self::ToolUse { .. } => "tool-use",
            Self::ToolReload => "tool-reload",
            Self::Bash { .. } => "bash",
            Self::BashLs { .. } => "bash-ls",
            Self::BashKill { .. } => "bash-kill",
            Self::BashLog { .. } => "bash-log",
            Self::Read { .. } => "read",
            Self::Edit { .. } => "edit",
            Self::Write { .. } => "write",
            Self::Tree { .. } => "tree",
            Self::WebFetch { .. } => "web-fetch",
            Self::TodoUpdate { .. } => "todo-update",
            Self::TodoDone => "todo-done",
            Self::TodoClear => "todo-clear",
            Self::SubAgent { .. } => "sub-agent",
            Self::SubTask { .. } => "sub-task",
            Self::SubCancel { .. } => "sub-cancel",
            Self::Event { .. } => "event",
            Self::EventLs { .. } => "event-ls",
            Self::EventCancel { .. } => "event-cancel",
            Self::Ask { .. } => "ask",
        }
    }

    /// 判断是否为只读操作
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::DocLs
                | Self::DocRead { .. }
                | Self::ToolLs { .. }
                | Self::ToolInfo { .. }
                | Self::BashLs { .. }
                | Self::BashLog { .. }
                | Self::Read { .. }
                | Self::Tree { .. }
                | Self::WebFetch { .. }
                | Self::SubTask { .. }
                | Self::EventLs { .. }
        )
    }

    /// 判断是否需要写入权限
    pub fn requires_write(&self) -> bool {
        matches!(
            self,
            Self::Edit { .. } | Self::Write { .. } | Self::TodoUpdate { .. }
        )
    }

    /// 判断是否为副作用操作（bash、工具调用、子 agent 等）
    pub fn has_side_effects(&self) -> bool {
        matches!(
            self,
            Self::Bash { .. }
                | Self::BashKill { .. }
                | Self::ToolUse { .. }
                | Self::SubAgent { .. }
                | Self::SubCancel { .. }
                | Self::Event { .. }
                | Self::EventCancel { .. }
        )
    }
}
