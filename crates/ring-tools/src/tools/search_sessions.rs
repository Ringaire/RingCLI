use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct SearchSessionsTool;

#[async_trait]
impl Tool for SearchSessionsTool {
    fn name(&self) -> &str { "search_sessions" }
    fn description(&self) -> &str { "Search sessions by title or message content." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let query = match input["query"].as_str() {
            Some(q) => q.to_lowercase(),
            None => return ToolResult::err("missing 'query'"),
        };

        let sessions = ring_core::session::list_sessions().await;
        let mut results = Vec::new();

        for meta in &sessions {
            let title_match = meta.title.as_deref()
                .map(|t| t.to_lowercase().contains(&query))
                .unwrap_or(false);
            if title_match {
                results.push(format!(
                    "{} | {}",
                    meta.id,
                    meta.title.as_deref().unwrap_or("(untitled)")
                ));
            }
        }

        if results.is_empty() { return ToolResult::ok_text("(no matching sessions)"); }
        ToolResult::ok_text(results.join("\n"))
    }
}
