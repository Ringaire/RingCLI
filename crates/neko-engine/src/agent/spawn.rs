// explore 工具：主 agent 派生独立子 agent 执行自包含子任务。
//
// 设计要点（对齐 nekocode-bun 并利用 Rust 结构优势）：
// - 子 agent 拥有独立 AgentContext（无共享记忆），任务全部信息由 `task` 字段传入
// - executor 自带 sub_agent_id，所有事件直接打标后流入主 bus（无需 bun 那样的双 bus 转发）
// - 深度限制：depth+1 < max_depth 时子 agent 仍带（更深的）explore，可多级编排；
//   达到叶层则用 SubToolRegistry 显式剥离 explore，防无限递归
// - role 选模：model 显式优先，否则按 role 从 catalog 选，再否则用当前模型
// - 子 agent 不持久化（persist=false）

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::debug;
use uuid::Uuid;

use neko_core::agent::{select_model_by_role, ModelCatalogEntry, ModelRole};
use neko_core::events::{EventBus, NekoEvent};
use neko_core::permissions::DefaultPermissionEngine;
use neko_core::tools::{
    AugmentedToolRegistry, ContentBlock, Message, MessageRole, SubToolRegistry, Tool, ToolContext,
    ToolRegistry, ToolResult,
};

use neko_providers::provider::Provider;

use crate::agent::context::AgentContext;
use crate::agent::executor::AgentExecutor;
use crate::agent::permission::PermissionSender;
use crate::agent::turn::TurnResult;

pub const SPAWN_TOOL_NAME: &str = "explore";
const DEFAULT_SUB_MAX_TURNS: usize = 10;

/// 派生子 agent 所需的全部能力（在编排器构建时捕获）。
#[derive(Clone)]
pub struct SpawnAgentTool {
    pub provider:      Arc<dyn Provider>,
    pub base_tools:    Arc<dyn ToolRegistry>,
    pub permissions:   Arc<Mutex<DefaultPermissionEngine>>,
    pub bus:           EventBus,
    pub permission_tx: Option<PermissionSender>,
    pub catalog:       Vec<ModelCatalogEntry>,
    pub current_model: String,
    pub depth:         usize,
    pub max_depth:     usize,
}

impl SpawnAgentTool {
    /// 构建子 agent 可用的工具集。
    /// 未达叶层：叠加更深一层的 spawn_agent（支持多级编排）。
    /// 叶层：剥离 spawn_agent。
    fn build_sub_tools(&self) -> Arc<dyn ToolRegistry> {
        if self.depth + 1 < self.max_depth {
            let deeper = SpawnAgentTool {
                depth: self.depth + 1,
                ..self.clone()
            };
            Arc::new(AugmentedToolRegistry::new(
                self.base_tools.clone(),
                vec![Arc::new(deeper)],
            ))
        } else {
            Arc::new(SubToolRegistry::new(
                self.base_tools.clone(),
                [SPAWN_TOOL_NAME.to_string()],
            ))
        }
    }

    fn resolve_model(&self, input: &Value) -> (String, Option<String>) {
        // 显式 model 优先
        if let Some(m) = input.get("model").and_then(|v| v.as_str()) {
            if !m.trim().is_empty() {
                return (m.to_string(), None);
            }
        }
        // 否则按 role 选
        if let Some(role_str) = input.get("role").and_then(|v| v.as_str()) {
            if let Ok(role) = role_str.parse::<ModelRole>() {
                let model = select_model_by_role(role, &self.catalog, &self.current_model);
                return (model, Some(role.as_str().to_string()));
            }
        }
        // 兜底：当前模型
        (self.current_model.clone(), None)
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        SPAWN_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Delegate a self-contained sub-task to a sub-agent that runs independently with its own \
         isolated message history. Specify `model` (exact id) or `role` (heavy/balanced/light/coding) \
         for automatic model selection. Pass ALL necessary context in the `task` field — sub-agents \
         share no memory or conversation history. Returns the sub-agent's final output."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Complete task description including all context the sub-agent needs"
                },
                "model": {
                    "type": "string",
                    "description": "Exact model id to use (overrides role)"
                },
                "role": {
                    "type": "string",
                    "enum": ["heavy", "balanced", "light", "coding"],
                    "description": "Auto-select best available model by capability level"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "Optional custom system prompt for the sub-agent"
                },
                "max_turns": {
                    "type": "integer",
                    "description": "Maximum agentic turns (default 10)",
                    "minimum": 1,
                    "maximum": 30
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        if self.depth >= self.max_depth {
            return ToolResult::err_code(
                format!("sub-agent depth limit ({}) reached — cannot spawn further", self.max_depth),
                "MAX_DEPTH",
            );
        }

        let task = match input.get("task").and_then(|v| v.as_str()) {
            Some(t) if !t.trim().is_empty() => t.to_string(),
            _ => return ToolResult::err("missing 'task'"),
        };

        let (model, role) = self.resolve_model(&input);
        let max_turns = input.get("max_turns")
            .and_then(|v| v.as_u64())
            .map(|n| (n as usize).clamp(1, 30))
            .unwrap_or(DEFAULT_SUB_MAX_TURNS);
        let sub_system = input.get("system_prompt").and_then(|v| v.as_str()).map(String::from);

        let sub_agent_id = Uuid::new_v4();
        debug!(%sub_agent_id, %model, depth = self.depth, "spawning sub-agent");

        // 通告派生
        self.bus.emit(NekoEvent::AgentSpawned {
            session_id:   ctx.session_id,
            sub_agent_id,
            role:         role.clone(),
            model:        model.clone(),
            task:         task.clone(),
        });

        // 子 agent 上下文：独立、不共享历史
        let mut sub_ctx = AgentContext::new(model.clone());
        sub_ctx.system = Some(sub_system.unwrap_or_else(|| default_sub_prompt(&self.current_model)));
        sub_ctx.add_message(Message::user_text(task.clone()));

        let sub_tools = self.build_sub_tools();

        let sub_executor = AgentExecutor::sub(
            self.provider.clone(),
            sub_tools,
            self.permissions.clone(),
            self.bus.clone(),
            ctx.session_id,
            ctx.cwd.clone(),
            self.permission_tx.clone(),
            sub_agent_id,
            max_turns,
        );

        let started = std::time::Instant::now();
        let result = sub_executor.run(&mut sub_ctx, ctx.signal.clone()).await;
        let elapsed = started.elapsed().as_secs_f64();

        match result {
            TurnResult::Error(e) => {
                return ToolResult::err_code(format!("sub-agent error: {e}"), "SUB_AGENT_ERROR");
            }
            TurnResult::Cancelled => {
                return ToolResult::err_code("sub-agent cancelled", "CANCELLED");
            }
            _ => {}
        }

        let output = collect_assistant_text(&sub_ctx.messages);
        let output = if output.trim().is_empty() {
            "(sub-agent completed with no text output)".to_string()
        } else {
            output
        };
        let tool_count = count_tool_uses(&sub_ctx.messages);

        let summary = format!(
            "[sub-agent model={} · {} tool calls · {:.1}s]\n\n{}",
            model, tool_count, elapsed, output
        );
        ToolResult::ok_text(summary)
    }
}

/// 提取消息序列中所有 assistant 文本块，拼接。
fn collect_assistant_text(messages: &[Message]) -> String {
    let mut parts = Vec::new();
    for m in messages {
        if m.role != MessageRole::Assistant {
            continue;
        }
        for b in &m.content {
            if let ContentBlock::Text { text } = b {
                if !text.trim().is_empty() {
                    parts.push(text.clone());
                }
            }
        }
    }
    parts.join("\n\n").trim().to_string()
}

/// 统计 assistant 发起的工具调用数。
fn count_tool_uses(messages: &[Message]) -> usize {
    messages.iter()
        .filter(|m| m.role == MessageRole::Assistant)
        .flat_map(|m| m.content.iter())
        .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
        .count()
}

fn default_sub_prompt(parent_model: &str) -> String {
    format!(
        "You are a sub-agent spawned to complete a single self-contained task. \
         You have no access to the parent conversation history — all needed context is in the task. \
         Complete the task using the available tools, then provide a clear, complete final answer. \
         (parent model: {parent_model})"
    )
}
