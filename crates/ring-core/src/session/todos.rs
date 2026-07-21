use serde::{Deserialize, Serialize};
use std::io;
use uuid::Uuid;

use super::paths;

// ── 类型 ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoSummary {
    pub pending:     usize,
    pub in_progress: usize,
    pub completed:   usize,
    pub cancelled:   usize,
}

impl TodoSummary {
    pub fn total(&self) -> usize {
        self.pending + self.in_progress + self.completed + self.cancelled
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

// ── IO ────────────────────────────────────────────────────────────────────────

fn todos_path(session_id: Uuid) -> std::path::PathBuf {
    paths::sessions_dir().join(format!("{}.todos.json", session_id))
}

pub async fn load_todo_summary(session_id: Uuid) -> Option<TodoSummary> {
    let raw = tokio::fs::read_to_string(todos_path(session_id)).await.ok()?;
    serde_json::from_str(&raw).ok()
}

pub async fn save_todo_summary(session_id: Uuid, summary: &TodoSummary) -> io::Result<()> {
    let data = serde_json::to_vec_pretty(summary)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    tokio::fs::write(todos_path(session_id), data).await
}
