use async_trait::async_trait;
use ring_core::{delete_memory, list_memory, save_memory, search_memory, MemoryEntry, MemoryType};
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use uuid::Uuid;

pub struct MemoryTool;

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str { "memory" }

    fn description(&self) -> &str {
        "Manage persistent memory across sessions. \
         Actions: save (add/update), list, search, delete. \
         Use this to remember facts, preferences, feedback, and project context for future conversations."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["save", "list", "search", "delete"],
                    "description": "Operation to perform"
                },
                "type": {
                    "type": "string",
                    "enum": ["user", "project", "feedback", "reference"],
                    "description": "Memory category (required for save)"
                },
                "title": {
                    "type": "string",
                    "description": "Short title / slug (required for save)"
                },
                "body": {
                    "type": "string",
                    "description": "Full memory content (required for save)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tags for save"
                },
                "id": {
                    "type": "string",
                    "description": "Memory UUID (required for delete)"
                },
                "query": {
                    "type": "string",
                    "description": "Search keyword (required for search)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let action = match input["action"].as_str() {
            Some(a) => a,
            None => return ToolResult::err("missing 'action'"),
        };

        match action {
            "save" => {
                let memory_type = match input["type"].as_str() {
                    Some("user")      => MemoryType::User,
                    Some("project")   => MemoryType::Project,
                    Some("feedback")  => MemoryType::Feedback,
                    Some("reference") => MemoryType::Reference,
                    _ => return ToolResult::err("missing or invalid 'type' (user|project|feedback|reference)"),
                };
                let title = match input["title"].as_str() {
                    Some(t) if !t.trim().is_empty() => t.to_string(),
                    _ => return ToolResult::err("missing 'title'"),
                };
                let body = match input["body"].as_str() {
                    Some(b) if !b.trim().is_empty() => b.to_string(),
                    _ => return ToolResult::err("missing 'body'"),
                };
                let tags: Vec<String> = input["tags"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                let mut entry = MemoryEntry::new(memory_type, title, body);
                entry.tags = tags;

                match save_memory(entry.clone()).await {
                    Ok(_) => ToolResult::ok_text(format!("memory saved: {} ({})", entry.title, entry.id)),
                    Err(e) => ToolResult::err(format!("failed to save memory: {}", e)),
                }
            }

            "list" => {
                let entries = list_memory().await;
                if entries.is_empty() {
                    return ToolResult::ok_text("(no memories)");
                }
                let out = entries.iter().map(|e| {
                    format!("[{}] [{:?}] {}: {}", e.id, e.memory_type, e.title, e.body)
                }).collect::<Vec<_>>().join("\n");
                ToolResult::ok_text(out)
            }

            "search" => {
                let query = match input["query"].as_str() {
                    Some(q) if !q.trim().is_empty() => q,
                    _ => return ToolResult::err("missing 'query'"),
                };
                let results = search_memory(query).await;
                if results.is_empty() {
                    return ToolResult::ok_text(format!("(no matches for '{}')", query));
                }
                let out = results.iter().map(|e| {
                    format!("[{}] [{:?}] {}: {}", e.id, e.memory_type, e.title, e.body)
                }).collect::<Vec<_>>().join("\n");
                ToolResult::ok_text(out)
            }

            "delete" => {
                let id_str = match input["id"].as_str() {
                    Some(s) => s,
                    None => return ToolResult::err("missing 'id'"),
                };
                let id = match Uuid::parse_str(id_str) {
                    Ok(u) => u,
                    Err(_) => return ToolResult::err(format!("invalid UUID: {}", id_str)),
                };
                match delete_memory(id).await {
                    Ok(true)  => ToolResult::ok_text(format!("memory {} deleted", id)),
                    Ok(false) => ToolResult::err(format!("memory {} not found", id)),
                    Err(e)    => ToolResult::err(format!("failed to delete memory: {}", e)),
                }
            }

            other => ToolResult::err(format!("unknown action: {}", other)),
        }
    }
}
