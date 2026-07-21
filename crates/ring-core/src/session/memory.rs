use serde::{Deserialize, Serialize};
use std::io;
use uuid::Uuid;

use super::paths;

// ── 类型 ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    User,
    Project,
    Feedback,
    Reference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id:           Uuid,
    pub memory_type:  MemoryType,
    pub title:        String,
    pub body:         String,
    pub created_at:   String,
    pub updated_at:   String,
    pub tags:         Vec<String>,
}

impl MemoryEntry {
    pub fn new(memory_type: MemoryType, title: impl Into<String>, body: impl Into<String>) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4(),
            memory_type,
            title: title.into(),
            body: body.into(),
            created_at: now.clone(),
            updated_at: now,
            tags: Vec::new(),
        }
    }
}

// ── 路径 ──────────────────────────────────────────────────────────────────────

fn memory_path() -> std::path::PathBuf {
    paths::cache_dir().join("memory.json")
}

// ── CRUD ──────────────────────────────────────────────────────────────────────

async fn read_all() -> Vec<MemoryEntry> {
    let path = memory_path();
    match tokio::fs::read_to_string(&path).await {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_)  => Vec::new(),
    }
}

async fn write_all(entries: &[MemoryEntry]) -> io::Result<()> {
    let path = memory_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let data = serde_json::to_vec_pretty(entries)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    tokio::fs::write(&path, data).await
}

pub async fn save_memory(entry: MemoryEntry) -> io::Result<()> {
    let mut entries = read_all().await;
    if let Some(existing) = entries.iter_mut().find(|e| e.id == entry.id) {
        *existing = entry;
    } else {
        entries.push(entry);
    }
    write_all(&entries).await
}

pub async fn delete_memory(id: Uuid) -> io::Result<bool> {
    let mut entries = read_all().await;
    let before = entries.len();
    entries.retain(|e| e.id != id);
    let removed = entries.len() < before;
    if removed { write_all(&entries).await?; }
    Ok(removed)
}

pub async fn list_memory() -> Vec<MemoryEntry> {
    read_all().await
}

pub async fn search_memory(query: &str) -> Vec<MemoryEntry> {
    let q = query.to_lowercase();
    read_all().await.into_iter().filter(|e| {
        e.title.to_lowercase().contains(&q)
            || e.body.to_lowercase().contains(&q)
            || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
    }).collect()
}

// ── System Prompt 注入 ────────────────────────────────────────────────────────

pub fn build_memory_prompt(entries: &[MemoryEntry]) -> String {
    if entries.is_empty() { return String::new(); }

    let mut out = String::from("## Memory\n");
    let groups = [
        (MemoryType::User,      "### User"),
        (MemoryType::Project,   "### Project"),
        (MemoryType::Feedback,  "### Feedback"),
        (MemoryType::Reference, "### Reference"),
    ];
    for (kind, header) in &groups {
        let section: Vec<_> = entries.iter().filter(|e| &e.memory_type == kind).collect();
        if section.is_empty() { continue; }
        out.push_str(header);
        out.push('\n');
        for e in section {
            out.push_str(&format!("- **{}**: {}\n", e.title, e.body));
        }
    }
    out
}
