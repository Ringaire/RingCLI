// 编排器：把主 agent 装配为 orchestrator —— 注入 explore / enter_plan_mode / exit_plan_mode 工具 + 编排系统提示词，
// 使主 agent 能把自包含子任务委派给按 role 选出的子 agent，并支持计划-执行工作流。

use std::path::PathBuf;
use std::sync::Arc;

use neko_core::agent::{ModelCatalogEntry, ModelRole};
use neko_core::events::EventBus;
use neko_core::permissions::DefaultPermissionEngine;
use neko_core::tools::{AugmentedToolRegistry, ToolRegistry};
use neko_providers::Provider;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::executor::AgentExecutor;
use crate::agent::permission::PermissionSender;
use crate::agent::plan::{EnterPlanModeTool, ExitPlanModeTool};
use crate::agent::spawn::SpawnAgentTool;

/// 子 agent 最大派生深度（含主 agent 之下的层数）。
pub const DEFAULT_MAX_DEPTH: usize = 2;

/// 构建主 agent 的 orchestrator executor：在基础工具集之上叠加 spawn_agent。
pub fn build_executor(
    tools:            Arc<dyn ToolRegistry>,
    permissions:      Arc<Mutex<DefaultPermissionEngine>>,
    bus:              EventBus,
    catalog:          Vec<ModelCatalogEntry>,
    current_model:    String,
    session_id:       Uuid,
    cwd:              PathBuf,
    max_tokens:       u64,
    provider:         Arc<dyn Provider>,
    perm_tx:          Option<PermissionSender>,
) -> AgentExecutor {
    let spawn = SpawnAgentTool {
        provider:      provider.clone(),
        base_tools:    tools.clone(),
        permissions:   permissions.clone(),
        bus:           bus.clone(),
        permission_tx: perm_tx.clone(),
        catalog:       catalog.clone(),
        current_model: current_model.clone(),
        depth:         0,
        max_depth:     DEFAULT_MAX_DEPTH,
    };

    let enter_plan = EnterPlanModeTool {
        permissions: permissions.clone(),
    };

    let exit_plan = ExitPlanModeTool {
        permissions: permissions.clone(),
    };

    let aug: Arc<dyn ToolRegistry> = Arc::new(AugmentedToolRegistry::new(
        tools.clone(),
        vec![Arc::new(spawn), Arc::new(enter_plan), Arc::new(exit_plan)],
    ));

    let mut exec = AgentExecutor::main(
        provider,
        aug,
        permissions.clone(),
        bus.clone(),
        session_id,
        cwd,
        perm_tx,
    );
    exec.max_output_tokens = max_tokens.clamp(1, u32::MAX as u64) as u32;
    exec
}

/// 构建编排系统提示词：在基础提示词后追加 sub-agent 模型目录与编排准则。
pub fn build_orchestrator_prompt(
    catalog:       &[ModelCatalogEntry],
    current_model: &str,
    base_prompt:   &str,
) -> String {
    let mut lines: Vec<String> = vec!["## Available sub-agent models".to_string()];

    for role in ModelRole::all() {
        let models: Vec<&str> = catalog.iter()
            .filter(|m| m.role == role)
            .map(|m| m.id.as_str())
            .take(4)
            .collect();
        if !models.is_empty() {
            lines.push(format!("**{}** ({}):", role.as_str(), role.description()));
            lines.push(format!("  {}", models.join(", ")));
        }
    }
    lines.push(format!("\nYou are running as: **{current_model}**"));

    let section = format!(
        "You are an orchestrator agent. You can delegate self-contained sub-tasks to specialized \
sub-agents using the `explore` tool.\n\n\
{}\n\n\
## Plan mode workflow\n\
Use `enter_plan_mode` when the user asks for architecture planning or task decomposition. \
This switches to plan mode where reads and explore are allowed but writes/bash require confirmation. \
After researching and writing a plan file, call `exit_plan_mode` to submit for approval.\n\n\
## Orchestration guidelines\n\
- Break complex tasks into independent sub-tasks and delegate them to appropriate models\n\
- Choose model role by task complexity: `heavy` for deep reasoning, `light` for simple lookups, `coding` for code work\n\
- Pass ALL necessary context in the `task` field — sub-agents have no shared memory or conversation history\n\
- Synthesize sub-agent outputs into a single cohesive final response\n\
- If a task is straightforward, handle it yourself without spawning sub-agents\n\
- Prefer `role` over explicit `model` unless you need a specific model's capabilities",
        lines.join("\n"),
    );

    if base_prompt.is_empty() {
        section
    } else {
        format!("{base_prompt}\n\n---\n\n{section}")
    }
}
