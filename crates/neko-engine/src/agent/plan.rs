// EnterPlanMode / ExitPlanMode 工具：Agent 自行进入/退出 plan 模式，
// 编写计划文件供用户审批后执行。

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use neko_core::permissions::{DefaultPermissionEngine, ModeName};
use neko_core::tools::{Tool, ToolContext, ToolResult};

pub const ENTER_PLAN_TOOL_NAME: &str = "enter_plan_mode";
pub const EXIT_PLAN_TOOL_NAME: &str = "exit_plan_mode";

/// 当前 plan 会话状态（全局，供 TUI / REPL 查询）
static PLAN_STATE: once_cell::sync::Lazy<tokio::sync::Mutex<Option<PlanSession>>> =
    once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(None));

#[derive(Clone, Debug)]
pub struct PlanSession {
    /// 计划文件路径
    pub plan_path: std::path::PathBuf,
    /// 进入 plan 前的权限模式（ExitPlanMode 时恢复）
    pub prev_mode: ModeName,
    /// 计划内容（ExitPlanMode 时填充）
    pub plan_content: Option<String>,
}

pub fn set_plan_state(state: Option<PlanSession>) {
    if let Ok(mut s) = PLAN_STATE.try_lock() {
        *s = state;
    }
}

pub fn get_plan_state() -> Option<PlanSession> {
    PLAN_STATE.try_lock().ok().and_then(|s| s.clone())
}

// ── EnterPlanMode ─────────────────────────────────────────────────────────────

pub struct EnterPlanModeTool {
    pub permissions: Arc<Mutex<DefaultPermissionEngine>>,
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        ENTER_PLAN_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Enter plan mode to research and write a plan. \
         Call this when the user asks for architecture planning or task decomposition. \
         Creates a plan file you can write to. \
         After writing the plan, call exit_plan_mode to submit for approval."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Brief description of what needs to be planned"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let task = input["task"].as_str().unwrap_or("plan");

        // 切换到 plan mode
        {
            let mut perms = self.permissions.lock().await;
            let prev = perms.mode();
            perms.set_mode(ModeName::Plan);

            // 生成计划文件路径
            let slug = slugify(task);
            let plan_dir = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("neko")
                .join("plans");
            let _ = std::fs::create_dir_all(&plan_dir);
            let plan_path = plan_dir.join(format!("{}.md", slug));

            set_plan_state(Some(PlanSession {
                plan_path: plan_path.clone(),
                prev_mode: prev,
                plan_content: None,
            }));

            debug!(?plan_path, prev = %prev, "entered plan mode");
        }

        let plan_path = get_plan_state().map(|s| s.plan_path).unwrap_or_default();

        ToolResult::ok_text(format!(
            "Entered **plan mode**.\n\
             Task: {}\n\
             Plan file: {}\n\n\
             You can now:\n\
             - Use `read_file`, `glob`, `grep`, `tree` etc. to research\n\
             - Use `explore` to delegate sub-tasks to sub-agents\n\
             - Write your plan to the plan file using `edit_file` or `write_file`\n\
             - Call `exit_plan_mode` when done to submit for approval",
            task,
            plan_path.display(),
        ))
    }
}

// ── ExitPlanMode ──────────────────────────────────────────────────────────────

pub struct ExitPlanModeTool {
    pub permissions: Arc<Mutex<DefaultPermissionEngine>>,
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        EXIT_PLAN_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Exit plan mode and submit the written plan for user approval. \
         Only call this AFTER writing the plan to the plan file. \
         Do NOT use AskUserQuestion to ask for approval — this tool handles that."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Brief summary of the plan for the user"
                }
            },
            "required": ["summary"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let summary = input["summary"].as_str().unwrap_or("Plan complete");

        let (plan_content, plan_path, prev_mode) = {
            let mut state = PLAN_STATE.lock().await;
            let session = match state.as_mut() {
                Some(s) => s,
                None => return ToolResult::err("not in plan mode — call enter_plan_mode first"),
            };

            let content = match std::fs::read_to_string(&session.plan_path) {
                Ok(c) => c,
                Err(_) => String::from("(plan file not found — use write_file to create it)"),
            };
            session.plan_content = Some(content.clone());

            let prev = session.prev_mode;
            let path = session.plan_path.clone();
            drop(state);

            {
                let mut perms = self.permissions.lock().await;
                perms.set_mode(prev);
            }

            set_plan_state(None);

            (content, path, prev)
        };

        ToolResult::ok_text(format!(
            "## Plan submitted for approval\n\n\
             **Summary:** {}\n\
             **Plan file:** {}\n\
             **Mode restored:** {}\n\n\
             ---\n\n\
             {}",
            summary,
            plan_path.display(),
            prev_mode,
            plan_content,
        ))
    }
}

fn slugify(s: &str) -> String {
    let s = s.to_lowercase();
    let s: String = s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let s: String = s.chars()
        .fold(String::new(), |mut acc, c| {
            if c == '-' && acc.ends_with('-') { /* skip duplicate */ }
            else { acc.push(c); }
            acc
        });
    s.trim_matches('-').to_string()
}
