// 系统提示词构建：注入环境、git 状态、工具与技能清单、Memory、AGENTS.md

use std::path::Path;

use neko_core::{build_memory_prompt, list_memory};
use neko_core::skills::SkillRegistry;
use neko_core::tools::ToolRegistry;

/// 从 cwd 开始向上查找 `AGENTS.md`，找到后读取内容。
/// 最多向上 10 层，遇到 git 根目录即停止（不跨仓库边界）。
pub fn read_agents_md(cwd: &Path) -> Option<String> {
    let mut dir = cwd.to_path_buf();
    for _ in 0..10 {
        let candidate = dir.join("AGENTS.md");
        if candidate.is_file() {
            return std::fs::read_to_string(&candidate).ok();
        }
        // 如果当前目录就是 git 根（含 .git/），不再往上
        if dir.join(".git").exists() {
            break;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    None
}

/// 构建发送给 LLM 的系统提示词。
/// 包含：身份、运行环境、cwd、git 状态、工具清单、技能清单、行为准则、Memory。
pub async fn build_system_prompt(
    cwd:    &Path,
    tools:  &dyn ToolRegistry,
    skills: &SkillRegistry,
    model:  &str,
    mode:   &str,
) -> String {
    let mut s = String::new();

    // ── 身份 ──
    s.push_str("You are neko, a terminal-based AI coding assistant. ");
    s.push_str("You help with software engineering tasks: writing code, debugging, refactoring, \
                running commands, and answering questions about codebases.\n\n");

    // ── 运行环境 ──
    s.push_str("# Environment\n");
    s.push_str(&format!("- neko version: {}\n", env!("CARGO_PKG_VERSION")));
    s.push_str(&format!("- Model: {}\n", model));
    s.push_str(&format!("- Permission mode: {}\n", mode));
    s.push_str(&format!("- Working directory: {}\n", cwd.display()));
    s.push_str(&format!("- Platform: {}\n", std::env::consts::OS));
    s.push_str(&format!("- Architecture: {}\n", std::env::consts::ARCH));
    let now = chrono::Local::now();
    s.push_str(&format!("- Current time: {}\n", now.format("%Y-%m-%d %H:%M:%S %z")));
    s.push('\n');

    // ── git 状态（容错：失败不影响提示词生成）──
    if let Some(git_info) = collect_git_info(cwd).await {
        s.push_str("# Git\n");
        s.push_str(&git_info);
        s.push('\n');
    }

    // ── AGENTS.md（项目级 agent 指令，优先级高于内置准则）──
    if let Some(agents_md) = read_agents_md(cwd) {
        let trimmed = agents_md.trim();
        if !trimmed.is_empty() {
            s.push_str("# Project Instructions (AGENTS.md)\n");
            s.push_str(trimmed);
            s.push_str("\n\n");
        }
    }

    // ── 工具清单 ──
    s.push_str("# Available Tools\n");
    let mut tool_list = tools.list();
    tool_list.sort_by(|a, b| a.name().cmp(b.name()));
    for tool in &tool_list {
        s.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
    }
    s.push('\n');

    // ── 工具附加 Prompt（工具可注入额外上下文到 system prompt）──
    let mut tool_prompts = String::new();
    for tool in &tool_list {
        if let Some(p) = tool.prompt() {
            tool_prompts.push_str(p);
            tool_prompts.push('\n');
        }
    }
    if !tool_prompts.trim().is_empty() {
        s.push_str("# Tool Context\n");
        s.push_str(tool_prompts.trim());
        s.push_str("\n\n");
    }

    // ── 技能清单（<available_skills> XML，AI 按需用 skill 工具加载）──
    let skills_xml = skills.build_available_skills();
    if !skills_xml.is_empty() {
        s.push_str("# Skills\n");
        s.push_str(&skills_xml);
        s.push_str("\n\n");
    }

    // ── Memory（用户在过去会话中记录的持久化信息）──
    let memories = list_memory().await;
    let mem_prompt = build_memory_prompt(&memories);
    if !mem_prompt.trim().is_empty() {
        s.push_str(&mem_prompt);
        s.push('\n');
    }

    // ── Plan 模式 ──
    s.push_str("# Plan Mode\n");
    s.push_str("When the user requests architecture planning or task decomposition, use ");
    s.push_str("`enter_plan_mode` to enter **plan mode**. This switches to a read-focused permission context ");
    s.push_str("where you can research, explore, and delegate sub-tasks via `explore`. ");
    s.push_str("Write your plan to the returned plan file path using `write_file` or `edit_file`.\n\n");
    s.push_str("Once the plan is written, call `exit_plan_mode` with a summary to submit for user approval. ");
    s.push_str("Do NOT use `ask` tool or chat to ask 'is this plan OK?' — `exit_plan_mode` handles that.\n\n");
    s.push_str("Plan mode workflow:\n");
    s.push_str("1. `enter_plan_mode(task=\"...\")` — enter plan mode, get plan file path\n");
    s.push_str("2. Research using read_file, glob, grep, explore, etc.\n");
    s.push_str("3. Write plan to the plan file\n");
    s.push_str("4. `exit_plan_mode(summary=\"...\")` — submit for approval\n\n");

    // ── 行为准则 ──
    s.push_str("# Guidelines\n");
    s.push_str("- Use tools to gather context before answering questions about the codebase.\n");
    s.push_str("- When editing files, prefer edit_file for targeted changes; use write_file only for new files or full rewrites.\n");
    s.push_str("- Read a file before editing it to ensure your edits match the actual content.\n");
    s.push_str("- Run read-only tools freely; mutating tools (bash, write_file, edit_file) may require user permission.\n");
    s.push_str("- Be concise. Avoid unnecessary preamble or summary unless asked.\n");
    s.push_str("- When a task is complete and verified, state it plainly. If something failed, report it honestly.\n");
    s.push_str("- Do not fabricate file paths, function names, or command output. Verify with tools.\n");

    s
}

/// 收集 git 信息：分支名 + 简要状态。全部容错。
async fn collect_git_info(cwd: &Path) -> Option<String> {
    // 检查是否在 git 仓库内
    let inside = run_git(cwd, &["rev-parse", "--is-inside-work-tree"]).await?;
    if inside.trim() != "true" {
        return None;
    }

    let mut out = String::new();

    if let Some(branch) = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).await {
        let branch = branch.trim();
        if !branch.is_empty() {
            out.push_str(&format!("- Current branch: {}\n", branch));
        }
    }

    if let Some(status) = run_git(cwd, &["status", "--porcelain"]).await {
        let lines: Vec<&str> = status.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            out.push_str("- Working tree: clean\n");
        } else {
            out.push_str(&format!("- Uncommitted changes: {} file(s)\n", lines.len()));
            for line in lines.iter().take(20) {
                out.push_str(&format!("  {}\n", line));
            }
            if lines.len() > 20 {
                out.push_str(&format!("  ... and {} more\n", lines.len() - 20));
            }
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

/// 运行 git 命令，返回 stdout（失败返回 None）。
async fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}
