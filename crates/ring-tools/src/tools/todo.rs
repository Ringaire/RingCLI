use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use ring_core::{save_todo_summary, TodoSummary};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub struct TodoTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id:       String,
    pub content:  String,
    pub status:   TodoStatus,
    pub priority: TodoPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

fn todos_path(ctx: &ToolContext) -> std::path::PathBuf {
    ring_core::session::paths::sessions_dir()
        .join(format!("{}.todos.json", ctx.session_id))
}

async fn load_todos(ctx: &ToolContext) -> Vec<TodoItem> {
    let path = todos_path(ctx);
    if let Ok(raw) = tokio::fs::read_to_string(&path).await {
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        Vec::new()
    }
}

async fn save_todos(ctx: &ToolContext, todos: &[TodoItem]) -> Result<(), String> {
    let path = todos_path(ctx);
    let raw = serde_json::to_string_pretty(todos).map_err(|e| e.to_string())?;
    tokio::fs::write(&path, raw.as_bytes()).await.map_err(|e| e.to_string())?;

    // 同步更新 summary（供 system prompt 注入）
    let summary = TodoSummary {
        pending:     todos.iter().filter(|t| t.status == TodoStatus::Pending).count(),
        in_progress: todos.iter().filter(|t| t.status == TodoStatus::InProgress).count(),
        completed:   todos.iter().filter(|t| t.status == TodoStatus::Done).count(),
        cancelled:   0,
    };
    let _ = save_todo_summary(ctx.session_id, &summary).await;
    Ok(())
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str { "todo" }
    fn description(&self) -> &str { "Manage a session-scoped TODO list. Actions: list, add, update, remove, clear." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "add", "update", "remove", "clear"] },
                "content": { "type": "string", "description": "Todo content (for add)" },
                "id": { "type": "string", "description": "Todo ID (for update/remove)" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "done"] },
                "priority": { "type": "string", "enum": ["high", "medium", "low"] }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let action = match input["action"].as_str() {
            Some(a) => a,
            None => return ToolResult::err("missing 'action'"),
        };

        match action {
            "list" => {
                let todos = load_todos(ctx).await;
                if todos.is_empty() { return ToolResult::ok_text("(no todos)"); }
                let out: String = todos.iter().map(|t| {
                    format!("[{}] [{}] [{}] {}", t.id, format!("{:?}", t.priority).to_lowercase(), format!("{:?}", t.status).to_lowercase(), t.content)
                }).collect::<Vec<_>>().join("\n");
                ToolResult::ok_text(out)
            }
            "add" => {
                let content = match input["content"].as_str() {
                    Some(c) => c.to_string(),
                    None => return ToolResult::err("missing 'content'"),
                };
                let priority: TodoPriority = input["priority"].as_str()
                    .and_then(|p| serde_json::from_value(Value::String(p.to_string())).ok())
                    .unwrap_or(TodoPriority::Medium);

                let mut todos = load_todos(ctx).await;
                let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
                todos.push(TodoItem { id: id.clone(), content, status: TodoStatus::Pending, priority });
                if let Err(e) = save_todos(ctx, &todos).await { return ToolResult::err(e); }
                ToolResult::ok_text(format!("added todo {}", id))
            }
            "update" => {
                let id = match input["id"].as_str() { Some(i) => i, None => return ToolResult::err("missing 'id'") };
                let mut todos = load_todos(ctx).await;
                let item = match todos.iter_mut().find(|t| t.id == id) {
                    Some(t) => t,
                    None => return ToolResult::err(format!("todo '{}' not found", id)),
                };
                if let Some(s) = input["status"].as_str() {
                    if let Ok(status) = serde_json::from_value(Value::String(s.to_string())) {
                        item.status = status;
                    }
                }
                if let Some(p) = input["priority"].as_str() {
                    if let Ok(priority) = serde_json::from_value(Value::String(p.to_string())) {
                        item.priority = priority;
                    }
                }
                if let Some(c) = input["content"].as_str() { item.content = c.to_string(); }
                if let Err(e) = save_todos(ctx, &todos).await { return ToolResult::err(e); }
                ToolResult::ok_text(format!("updated todo {}", id))
            }
            "remove" => {
                let id = match input["id"].as_str() { Some(i) => i, None => return ToolResult::err("missing 'id'") };
                let mut todos = load_todos(ctx).await;
                let before = todos.len();
                todos.retain(|t| t.id != id);
                if todos.len() == before { return ToolResult::err(format!("todo '{}' not found", id)); }
                if let Err(e) = save_todos(ctx, &todos).await { return ToolResult::err(e); }
                ToolResult::ok_text(format!("removed todo {}", id))
            }
            "clear" => {
                if let Err(e) = save_todos(ctx, &[]).await { return ToolResult::err(e); }
                ToolResult::ok_text("cleared all todos")
            }
            other => ToolResult::err(format!("unknown action: {}", other)),
        }
    }
}
