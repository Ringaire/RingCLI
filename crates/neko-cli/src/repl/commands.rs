//! Slash 命令：解析、执行分派、帮助，以及 TUI 内联补全所需的元数据。
//!
//! `COMMANDS` 是命令元数据的**单一来源**——帮助文本、`/` 输入时的补全建议、
//! 参数提示都从这里派生，避免多处重复、不一致。
//!
//! 命令的实际副作用分两类：
//!   - 纯展示/会话控制（help / sessions / memory …）在此或调用方就地处理；
//!   - 需要改运行时状态（mode / model / resume / compact …）通过
//!     [`CommandOutcome`] 回传给主循环（TUI `app.rs` 或 plain `repl/mod.rs`）执行。

use anyhow::Result;

use neko_core::permissions::ModeName;
use neko_core::skills::SkillRegistry;
use neko_core::{delete_memory, list_memory, search_memory};
use uuid::Uuid;

// ── 命令元数据（单一来源）────────────────────────────────────────────────────

/// 一条 slash 命令的静态元数据。
pub struct CommandMeta {
    /// 规范名（不含前导 `/`）。
    pub name:        &'static str,
    /// 帮助 / 补全里展示的一行描述。
    pub description: &'static str,
    /// 参数提示（命令名敲完后展示），无参数则为 `None`。
    pub arg_hint:    Option<&'static str>,
}

/// 全部内置命令。顺序即帮助与补全的展示顺序。
pub const COMMANDS: &[CommandMeta] = &[
    CommandMeta { name: "help",     description: "Show command list",                     arg_hint: None },
    CommandMeta { name: "mode",     description: "Switch permission mode",                arg_hint: Some("ask|edit|plan|build|agent") },
    CommandMeta { name: "model",    description: "Show or switch model",                  arg_hint: Some("[provider/model-id]") },
    CommandMeta { name: "connect",  description: "Configure provider connection",         arg_hint: Some("[provider] [key] [url]") },
    CommandMeta { name: "think",    description: "Control extended thinking (on/off/budget)", arg_hint: Some("on|off [budget]") },
    CommandMeta { name: "thinking", description: "Toggle reasoning process display (fold/expand)", arg_hint: None },
    CommandMeta { name: "effort",   description: "Set reasoning effort level",     arg_hint: Some("[low|medium|high|max]") },
    CommandMeta { name: "resume",   description: "Resume / manage saved sessions",        arg_hint: Some("[sessions|ls|<session-uuid>]") },
    CommandMeta { name: "compact",  description: "Summarize & compact the conversation",  arg_hint: None },
    CommandMeta { name: "clear",    description: "Clear the screen / chat",               arg_hint: None },
    CommandMeta { name: "new",      description: "Start a new conversation session",      arg_hint: None },
    CommandMeta { name: "memory",   description: "List / search / delete memories",       arg_hint: Some("[search <q> | rm <id>]") },
    CommandMeta { name: "plan",     description: "Enter plan mode for architecture planning", arg_hint: Some("[description]") },
    CommandMeta { name: "init",     description: "Generate AGENTS.md for this project",  arg_hint: None },
    CommandMeta { name: "loop",     description: "Autonomous loop — agent works toward a goal", arg_hint: Some("<goal> [max_turns] | stop | status") },
    CommandMeta { name: "reload",   description: "Reload config + providers + skills",       arg_hint: None },
    CommandMeta { name: "quit",     description: "Exit neko",                             arg_hint: None },
];

/// 命令别名 → 规范名（用于解析，不进入补全/帮助列表）。
const ALIASES: &[(&str, &str)] = &[
    ("h", "help"),
    ("?", "help"),
    ("cls", "clear"),
    ("mem", "memory"),
    ("exit", "quit"),
    ("q", "quit"),
];

fn canonical(cmd: &str) -> &str {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == cmd)
        .map(|(_, name)| *name)
        .unwrap_or(cmd)
}

// ── 命令处理结果 ──────────────────────────────────────────────────────────────

/// `handle` 的结果：要么就地处理完（`Handled`），要么回传给主循环执行。
pub enum CommandOutcome {
    /// 不是命令，原样作为 prompt 发送。
    NotACommand(String),
    /// 展开技能为 prompt 发送。
    RunSkill { prompt: String },
    /// 切换权限模式。
    SwitchMode(ModeName),
    /// 打开交互式模式选择器（/mode 无参数时）。
    OpenModePicker,
    /// 切换模型（`provider/model` 或裸 `model`）。
    SwitchModel(String),
    /// 打开交互式模型选择器（/model 无参数时）。
    OpenModelPicker,
    /// 打开 `/connect` provider 配置向导（无参数时）。
    OpenProviderSetup,
    /// `/connect <provider> <key> [url]` 快速配置。
    QuickConnect { provider: String, api_key: Option<String>, base_url: Option<String> },
    /// 控制 extended thinking（on/off + budget）。
    SwitchThinking { enabled: bool, budget: Option<u32> },
    /// 切换 reasoning 显示（折叠/展开）。
    ToggleThinkingDisplay,
    /// 设置 reasoning effort 级别（low/medium/high/max）。
    SetEffort(String),
    /// 清屏 / 清空对话。
    Clear,
    /// 压缩上下文。
    Compact,
    /// 恢复指定会话。
    Resume(Uuid),
    /// 新开会话（清空当前对话，从新会话开始）。
    NewSession,
    /// 打开会话选择器。
    OpenSessionPicker,
    /// 退出。
    Quit,
    /// 进入 plan 模式。
    EnterPlan(String),
    /// 生成 AGENTS.md：把生成任务作为 prompt 发给 agent。
    InitAgentsMd,
    /// 启动自主循环模式。
    LoopStart { goal: String, max_turns: u32 },
    /// 停止循环。
    LoopStop,
    /// 查看循环状态。
    LoopStatus,
    /// 热重载配置 + provider + skill。
    Reload,
    /// 已就地处理（或需主循环按命令名做 async 收尾，如 /sessions、/memory）。
    Handled,
}

// ── 解析与分派 ────────────────────────────────────────────────────────────────

/// 解析一行输入。非 `/` 开头视为普通消息。
pub fn handle(text: &str, skills: &SkillRegistry) -> CommandOutcome {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return CommandOutcome::NotACommand(text.to_string());
    }

    let body = &trimmed[1..];
    let mut parts = body.splitn(2, char::is_whitespace);
    let raw_cmd = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    let lower_cmd = raw_cmd.to_lowercase();
    let cmd = canonical(&lower_cmd);

    // `/` 开头但不是已知命令/skill，且 token 里含路径字符（`/` `\` `.`）→ 当普通消息发送，
    // 不要把粘贴的文件路径（如 `/home/user/x.rs`、`/usr/bin/ls -la`）误吞成命令。
    let is_known = COMMANDS.iter().any(|c| c.name == cmd) || skills.get(cmd).is_some();
    if !is_known && raw_cmd.contains(['/', '\\', '.']) {
        return CommandOutcome::NotACommand(text.to_string());
    }

    match cmd {
        "help" => {
            print_help(skills);
            CommandOutcome::Handled
        }
        "mode" => {
            if rest.is_empty() {
                CommandOutcome::OpenModePicker
            } else {
                match rest.parse::<ModeName>() {
                    Ok(mode) => CommandOutcome::SwitchMode(mode),
                    Err(_) => {
                        println!("usage: /mode ask|edit|plan|build|agent");
                        CommandOutcome::Handled
                    }
                }
            }
        }
        "model" => {
            if rest.is_empty() {
                CommandOutcome::OpenModelPicker
            } else {
                CommandOutcome::SwitchModel(rest.to_string())
            }
        }
        "connect" => {
            let mut parts = rest.split_whitespace();
            match parts.next() {
                None => CommandOutcome::OpenProviderSetup,
                Some(provider) => CommandOutcome::QuickConnect {
                    provider: provider.to_lowercase(),
                    api_key:  parts.next().map(str::to_string),
                    base_url: parts.next().map(str::to_string),
                },
            }
        }
        "think" => {
            let mut parts = rest.split_whitespace();
            let sub = parts.next().unwrap_or("").to_lowercase();
            match sub.as_str() {
                "on" => {
                    let budget: Option<u32> = parts.next().and_then(|s| s.parse().ok());
                    CommandOutcome::SwitchThinking { enabled: true, budget }
                }
                "off" => {
                    CommandOutcome::SwitchThinking { enabled: false, budget: None }
                }
                "" => {
                    println!("usage: /think on|off [budget]");
                    CommandOutcome::Handled
                }
                _ => {
                    if let Ok(n) = sub.parse::<u32>() {
                        CommandOutcome::SwitchThinking { enabled: true, budget: Some(n) }
                    } else {
                        println!("usage: /think on|off [budget]");
                        CommandOutcome::Handled
                    }
                }
            }
        }
        "thinking" => CommandOutcome::ToggleThinkingDisplay,
        "effort" => {
            let level = rest.trim().to_lowercase();
            match level.as_str() {
                "low" | "medium" | "high" | "max" | "off" => CommandOutcome::SetEffort(level),
                "" => {
                    println!("usage: /effort low|medium|high|max|off");
                    CommandOutcome::Handled
                }
                _ => {
                    println!("usage: /effort low|medium|high|max|off");
                    CommandOutcome::Handled
                }
            }
        }
        "resume" => {
            let sub = rest.to_lowercase();
            if sub.is_empty() || sub == "sessions" || sub == "ls" {
                CommandOutcome::OpenSessionPicker
            } else {
                match Uuid::parse_str(rest) {
                    Ok(id) => CommandOutcome::Resume(id),
                    Err(_) => {
                        println!("usage: /resume [sessions|ls|<session-uuid>]");
                        CommandOutcome::Handled
                    }
                }
            }
        },
        "new" => CommandOutcome::NewSession,
        "clear" => CommandOutcome::Clear,
        "compact" => CommandOutcome::Compact,
        // memory 子命令在主循环里异步执行。
        "memory" => CommandOutcome::Handled,
        "plan" => CommandOutcome::EnterPlan(rest.to_string()),
        "init" => CommandOutcome::InitAgentsMd,
        "loop" => {
            let rest = rest.trim();
            if rest.is_empty() {
                println!("usage: /loop <goal> [max_turns]");
                println!("       /loop stop");
                println!("       /loop status");
                return CommandOutcome::Handled;
            }
            if rest == "stop" || rest == "cancel" {
                return CommandOutcome::LoopStop;
            }
            if rest == "status" {
                return CommandOutcome::LoopStatus;
            }
            // 最后一个 token 是 1-50 的纯数字时作为 max_turns
            let max_cap = neko_core::session::loop_state::MAX_LOOP_TURNS;
            let (goal, max_turns) = {
                let mut parts = rest.rsplitn(2, ' ');
                let last = parts.next().unwrap_or("");
                if let Ok(n) = last.parse::<u32>() {
                    if n >= 1 && n <= max_cap {
                        let g = parts.next().unwrap_or("").trim().to_string();
                        if !g.is_empty() { (g, n) } else { (rest.to_string(), 20) }
                    } else {
                        (rest.to_string(), 20)
                    }
                } else {
                    (rest.to_string(), 20)
                }
            };
            CommandOutcome::LoopStart { goal, max_turns }
        }
        "quit" => CommandOutcome::Quit,
        "reload" => CommandOutcome::Reload,
        // 其余：尝试作为技能名。
        other => {
            if let Some(skill) = skills.get(other) {
                let mut prompt = skill.content.clone();
                if !rest.is_empty() {
                    prompt.push_str("\n\nUser arguments: ");
                    prompt.push_str(rest);
                }
                CommandOutcome::RunSkill { prompt }
            } else {
                println!("unknown command or skill: /{other}  (try /help)");
                CommandOutcome::Handled
            }
        }
    }
}

// ── 帮助 ──────────────────────────────────────────────────────────────────────

fn print_help(skills: &SkillRegistry) {
    println!("Commands:");
    for c in COMMANDS {
        let usage = match c.arg_hint {
            Some(h) => format!("/{} {}", c.name, h),
            None => format!("/{}", c.name),
        };
        println!("  {:<28} {}", usage, c.description);
    }
    let skill_list = skills.list();
    if !skill_list.is_empty() {
        println!("Skills:");
        for s in skill_list {
            println!("  /{:<26} {}", s.name, s.description);
        }
    }
}

// ── 异步收尾（主循环在 `Handled` 后按命令名调用）──────────────────────────────

/// 已保存会话的格式化展示行（单一来源，供 plain REPL 与 TUI 复用）。
/// 无会话时返回单行 `(no sessions)`。
pub async fn session_lines() -> Vec<String> {
    let sessions = neko_core::session::list_sessions().await;
    if sessions.is_empty() {
        return vec!["(no sessions)".to_string()];
    }
    sessions.iter().map(|s| {
        let when = chrono::DateTime::from_timestamp_millis(s.updated_at)
            .map(|d: chrono::DateTime<chrono::Utc>| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_string());
        format!(
            "{} | {} | {} msgs | {}",
            s.id,
            s.title.as_deref().unwrap_or("(untitled)"),
            s.message_count,
            when,
        )
    }).collect()
}

/// 列出已保存会话（plain 模式打印）。
pub async fn list_sessions() -> Result<()> {
    for line in session_lines().await {
        println!("{line}");
    }
    Ok(())
}

/// 处理 `/memory` 子命令：`search <q>`、`rm <id>`，或裸 `/memory` 列出全部。
pub async fn handle_memory(rest: &str) -> Result<()> {
    if let Some(id_str) = rest.strip_prefix("rm").map(str::trim).filter(|s| !s.is_empty()) {
        match Uuid::parse_str(id_str) {
            Ok(id) => {
                if delete_memory(id).await? {
                    println!("[memory {id} deleted]");
                } else {
                    println!("[memory {id} not found]");
                }
            }
            Err(_) => println!("usage: /memory rm <uuid>"),
        }
        return Ok(());
    }

    let entries = if let Some(q) = rest.strip_prefix("search").map(str::trim).filter(|s| !s.is_empty()) {
        search_memory(q).await
    } else {
        list_memory().await
    };

    if entries.is_empty() {
        println!("(no memories)");
    } else {
        for e in &entries {
            println!("[{:?}] {} — {}  ({})", e.memory_type, e.title, e.body, e.id);
        }
    }
    Ok(())
}

// ── 内联补全（供 TUI 使用）────────────────────────────────────────────────────

/// 一条补全建议。
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// 接受后填入输入框的完整值，如 `/model`。
    pub value:       String,
    /// 列表中展示的标签，如 `/model`。
    pub label:       String,
    /// 一行描述。
    pub description: String,
}

/// 输入 `/前缀`（尚未输入空格）时的命令 + 技能建议。
pub fn command_suggestions(input: &str, skills: &SkillRegistry) -> Vec<Suggestion> {
    if !input.starts_with('/') || input[1..].contains(char::is_whitespace) {
        return Vec::new();
    }
    let prefix = input[1..].to_lowercase();
    let mut out = Vec::new();

    for c in COMMANDS {
        if c.name.starts_with(&prefix) {
            out.push(Suggestion {
                value:       format!("/{}", c.name),
                label:       format!("/{}", c.name),
                description: c.description.to_string(),
            });
        }
    }
    for s in skills.list() {
        let lname = s.name.to_lowercase();
        let is_builtin = COMMANDS.iter().any(|c| c.name == lname);
        if lname.starts_with(&prefix) && !is_builtin {
            out.push(Suggestion {
                value:       format!("/{}", s.name),
                label:       format!("/{}", s.name),
                description: s.description.clone(),
            });
        }
    }
    out
}

/// 构建 `/init` 发给 agent 的 prompt，让它分析项目并生成 AGENTS.md。
pub fn build_init_prompt(cwd: &std::path::Path) -> String {
    format!(
        "Analyze the project at `{}` and generate an `AGENTS.md` file in that directory.\n\n\
        AGENTS.md is the project-level instruction file for AI coding assistants (equivalent to CLAUDE.md). \
        It should be concise and actionable — not a tutorial.\n\n\
        Include:\n\
        - Project overview (1-2 sentences: what it is, language/stack)\n\
        - Crate/module structure (for Rust: list crates and their roles)\n\
        - Build & test commands (`cargo build`, `cargo test`, etc.)\n\
        - Key architectural decisions and invariants the AI must respect\n\
        - Coding conventions specific to this project\n\
        - What NOT to do (common pitfalls, files not to modify, etc.)\n\n\
        Use `tree`, `read_file`, `glob`, and `grep` to understand the project first. \
        Then write the file with `write_file`. \
        Keep it under 150 lines. No fluff.",
        cwd.display()
    )
}

/// 行内幽灵补全：首个候选相对当前输入多出的后缀（用于灰字提示）。
pub fn inline_ghost(input: &str, suggestions: &[Suggestion]) -> String {
    if !input.starts_with('/') {
        return String::new();
    }
    match suggestions.first() {
        Some(s) if s.value.starts_with(input) && s.value != input => s.value[input.len()..].to_string(),
        _ => String::new(),
    }
}

// ── 压缩相关 ──────────────────────────────────────────────────────────────────

/// compact 摘要请求的 system prompt。
pub const COMPACT_SYSTEM_PROMPT: &str = "\
You are compacting the earlier part of a coding agent conversation to save context window space.
Summarize the assistant/tool work into a briefing the agent can resume from.
Write under these headings, omitting any heading that has no content:

## Goal
The user's task and intent, in their own words.

## Decisions
Key choices made and why — so they are not re-derived or reversed.

## Files & Code
Files read or modified, with specific facts: function signatures, line ranges, exact edits applied.

## Commands & Results
Commands run (builds, tests, shell) and relevant outcomes — what passed, failed, error text that matters.

## Errors & Fixes
Problems encountered and how they were resolved (or not), so the same dead ends are not repeated.

## Pending
What is still in progress or unstarted, and the single most concrete next step.

Rules: use bullet points and fragments, not prose. Preserve identifiers, paths, and numbers exactly. \
Do NOT invent anything not present in the conversation.";

/// 将消息列表渲染为给摘要模型看的文字稿。
pub fn render_compact_transcript(messages: &[neko_core::tools::Message]) -> String {
    use neko_core::tools::MessageRole;
    let mut out = String::new();
    for msg in messages {
        let text = msg_text(msg);
        match msg.role {
            MessageRole::User => {
                out.push_str("[user]\n");
                out.push_str(&text);
                out.push_str("\n\n");
            }
            MessageRole::Assistant => {
                out.push_str("[assistant]\n");
                out.push_str(&text);
                out.push_str("\n\n");
            }
            MessageRole::ToolResult => {
                // 工具结果：只取前 500 字符（大输出已有缓存机制，摘要里不需要全量）
                let preview = if text.len() > 500 { &text[..500] } else { &text };
                out.push_str("[tool result]\n");
                out.push_str(preview);
                if text.len() > 500 { out.push_str("\n…(truncated)"); }
                out.push_str("\n\n");
            }
        }
    }
    out
}

fn msg_text(msg: &neko_core::tools::Message) -> String {
    use neko_core::tools::ContentBlock;
    msg.content.iter().filter_map(|b| match b {
        ContentBlock::Text { text } => Some(text.as_str()),
        _ => None,
    }).collect::<Vec<_>>().join("\n")
}

/// 检测消息列表开头是否已有摘要消息对，若有则剥离并返回其摘要文本。
/// 返回 `(prior_summary, new_messages_slice)`。
pub fn split_for_compact(messages: &[neko_core::tools::Message]) -> (Option<String>, &[neko_core::tools::Message]) {
    use neko_core::tools::MessageRole;
    if let Some(first) = messages.first() {
        if first.role == MessageRole::User
            && first.content.iter().any(|b| matches!(b, neko_core::tools::ContentBlock::Text { text } if text.starts_with("Please summarize")))
        {
            if let Some(second) = messages.get(1) {
                let text = msg_text(second);
                if text.starts_with("[Conversation Summary]") {
                    let prior = text["[Conversation Summary]".len()..].trim().to_string();
                    return (Some(prior), &messages[2..]);
                }
            }
        }
    }
    (None, messages)
}

/// 构造压缩后的摘要消息对（User + Assistant），保证消息序列以 User role 开始。
/// 若有历史摘要则以 `---` 分隔拼接。
pub fn build_compact_message(prior: Option<&str>, new_summary: &str) -> Vec<neko_core::tools::Message> {
    use neko_core::tools::{ContentBlock, MessageRole};
    let text = match prior {
        Some(p) => format!("[Conversation Summary]\n{p}\n\n---\n\n{new_summary}"),
        None    => format!("[Conversation Summary]\n{new_summary}"),
    };
    vec![
        neko_core::tools::Message::new(MessageRole::User, vec![ContentBlock::Text {
            text: "Please summarize the conversation below into a compact briefing using the requested format.".into()
        }]),
        neko_core::tools::Message::new(MessageRole::Assistant, vec![ContentBlock::Text { text }]),
    ]
}

/// 命令名敲完（已含空格）后的参数提示。
pub fn argument_hint(input: &str) -> Option<&'static str> {
    if !input.starts_with('/') {
        return None;
    }
    let body = &input[1..];
    if !body.contains(char::is_whitespace) {
        return None; // 还在敲命令名
    }
    let first = body.split_whitespace().next().unwrap_or("").to_lowercase();
    let name = canonical(&first);
    COMMANDS.iter().find(|c| c.name == name).and_then(|c| c.arg_hint)
}
