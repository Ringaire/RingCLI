use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct ListSessionsTool;

#[async_trait]
impl Tool for ListSessionsTool {
    fn name(&self) -> &str { "list_sessions" }
    fn description(&self) -> &str { "List all saved sessions." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "Max sessions to list (default 20)" }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let limit = input["limit"].as_u64().unwrap_or(20) as usize;
        let sessions = ring_core::session::list_sessions().await;
        if sessions.is_empty() { return ToolResult::ok_text("(no sessions)"); }
        let out: String = sessions.iter().take(limit).map(|s| {
            format!(
                "{} | {} | {} msgs | {}",
                s.id,
                s.title.as_deref().unwrap_or("(untitled)"),
                s.message_count,
                chrono::DateTime::from_timestamp_millis(s.updated_at)
                    .map(|d: chrono::DateTime<chrono::Utc>| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".to_string()),
            )
        }).collect::<Vec<_>>().join("\n");
        ToolResult::ok_text(out)
    }
}
