use async_trait::async_trait;
use neko_core::permissions::ModeName;
use neko_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

/// Agent 自主切换权限模式的工具。
///
/// 当 agent 检测到用户请求属于架构规划 / 任务拆解时，可调用此工具
/// 自动切换到 plan 模式，无需用户手动输入 `/mode plan`。
pub struct SetModeTool;

#[async_trait]
impl Tool for SetModeTool {
    fn name(&self) -> &str { "set_mode" }

    fn description(&self) -> &str {
        "Switch the permission mode. Use 'plan' when the user asks for architecture planning, \
         task decomposition, or multi-step design — read-only tools will be available while \
         writes and shell require confirmation. Use 'build' to return to full-auto after planning."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["ask", "edit", "plan", "build", "agent"],
                    "description": "The permission mode to switch to."
                }
            },
            "required": ["mode"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult {
        let mode_str = match args.get("mode").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::err("missing required parameter: mode"),
        };

        let mode: ModeName = match mode_str.parse() {
            Ok(m) => m,
            Err(e) => return ToolResult::err(e),
        };

        // 通过 context 的 on_mode_switch 回调（如果有的话），或直接返回
        // 让调用方处理模式切换。ToolResult 里放模式信息，executor 层面处理。
        let prev_mode = ctx.mode.clone().unwrap_or_default();
        ToolResult::ok(format!(
            "Switched permission mode: {prev_mode} → {mode}\n\
             In {mode} mode: read/search tools are free; \
             {}",
            match mode {
                ModeName::Build => "bash, file write, and edit are auto-approved.",
                ModeName::Edit  => "file write/edit auto-approved, bash disabled.",
                ModeName::Plan  => "writes and bash require user confirmation.",
                ModeName::Ask   => "all writes and bash are denied (read-only).",
                ModeName::Agent => "fully autonomous — all permission checks skipped.",
            }
        ))
    }
}
