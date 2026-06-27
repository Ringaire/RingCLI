pub mod memory;
pub mod paths;
pub mod todos;
pub mod loop_state;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::tools::{ContentBlock, Message, MessageRole};

// ── SessionMeta ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id:            Uuid,
    pub title:         Option<String>,
    pub cwd:           PathBuf,
    pub created_at:    i64,
    pub updated_at:    i64,
    pub message_count: usize,
    pub model:         Option<String>,
}

// ── Session ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Session {
    pub meta:     SessionMeta,
    pub messages: Vec<Message>,
}

// ── 内存中待写入的会话（尚未落盘）────────────────────────────────────────────

static PENDING: once_cell::sync::Lazy<Arc<Mutex<HashMap<Uuid, SessionMeta>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

// ── 公共 API ──────────────────────────────────────────────────────────────────

pub async fn init_dirs() -> std::io::Result<()> {
    let sessions = paths::sessions_dir();
    tokio::fs::create_dir_all(&sessions).await?;
    tokio::fs::create_dir_all(paths::config_dir()).await?;
    tokio::fs::create_dir_all(paths::cache_dir()).await?;
    tokio::fs::create_dir_all(paths::state_dir()).await?;
    tokio::fs::create_dir_all(paths::skills_dir()).await
}

pub async fn create_session(cwd: PathBuf, model: Option<String>) -> Session {
    let now = chrono::Utc::now().timestamp_millis();
    let meta = SessionMeta {
        id: Uuid::new_v4(),
        title: None,
        cwd,
        created_at: now,
        updated_at: now,
        message_count: 0,
        model,
    };
    PENDING.lock().await.insert(meta.id, meta.clone());
    Session { meta, messages: Vec::new() }
}

pub async fn append_message(session_id: Uuid, msg: Message) -> std::io::Result<()> {
    let sessions_dir = paths::sessions_dir();

    // 首次写入时落盘 meta 和空 jsonl
    let mut pending = PENDING.lock().await;
    if let Some(meta) = pending.remove(&session_id) {
        let meta_path = sessions_dir.join(format!("{}.meta.json", session_id));
        let jsonl_path = sessions_dir.join(format!("{}.jsonl", session_id));
        tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;
        tokio::fs::write(&jsonl_path, b"").await?;
    }
    drop(pending);

    // 追加消息行
    let jsonl_path = sessions_dir.join(format!("{}.jsonl", session_id));
    let mut line = serde_json::to_string(&msg)?;
    line.push('\n');
    use tokio::io::AsyncWriteExt;
    let mut f = tokio::fs::OpenOptions::new()
        .create(true).append(true)
        .open(&jsonl_path).await?;
    f.write_all(line.as_bytes()).await?;

    // 更新 meta
    let meta_path = sessions_dir.join(format!("{}.meta.json", session_id));
    if let Ok(raw) = tokio::fs::read_to_string(&meta_path).await {
        if let Ok(mut meta) = serde_json::from_str::<SessionMeta>(&raw) {
            meta.updated_at = chrono::Utc::now().timestamp_millis();
            meta.message_count += 1;
            if meta.title.is_none() {
                if let MessageRole::User = msg.role {
                    if let Some(ContentBlock::Text { text }) = msg.content.first() {
                        meta.title = Some(text.chars().take(60).collect());
                    }
                }
            }
            tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;
        }
    }

    Ok(())
}

pub async fn list_sessions() -> Vec<SessionMeta> {
    let sessions_dir = paths::sessions_dir();
    let mut metas = Vec::new();

    if let Ok(mut rd) = tokio::fs::read_dir(&sessions_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(raw) = tokio::fs::read_to_string(&p).await {
                    if let Ok(m) = serde_json::from_str::<SessionMeta>(&raw) {
                        metas.push(m);
                    }
                }
            }
        }
    }

    // 按更新时间倒序（最近的在前）
    metas.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    metas
}

pub async fn load_session(id: Uuid) -> Option<Session> {
    let sessions_dir = paths::sessions_dir();
    let meta_path  = sessions_dir.join(format!("{}.meta.json", id));
    let jsonl_path = sessions_dir.join(format!("{}.jsonl",      id));

    let meta_raw = tokio::fs::read_to_string(&meta_path).await.ok()?;
    let meta: SessionMeta = serde_json::from_str(&meta_raw).ok()?;

    let jsonl_raw = tokio::fs::read_to_string(&jsonl_path).await.unwrap_or_default();
    let messages = jsonl_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Message>(l).ok())
        .collect();

    Some(Session { meta, messages })
}

pub async fn replace_messages(session_id: Uuid, messages: &[Message]) -> std::io::Result<()> {
    let jsonl_path = paths::sessions_dir().join(format!("{}.jsonl", session_id));
    let mut content = String::new();
    for msg in messages {
        content.push_str(&serde_json::to_string(msg)?);
        content.push('\n');
    }
    tokio::fs::write(&jsonl_path, content.as_bytes()).await
}

pub async fn rename_session(session_id: Uuid, title: String) -> std::io::Result<()> {
    let meta_path = paths::sessions_dir().join(format!("{}.meta.json", session_id));
    if let Ok(raw) = tokio::fs::read_to_string(&meta_path).await {
        if let Ok(mut meta) = serde_json::from_str::<SessionMeta>(&raw) {
            meta.title = Some(title);
            tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;
        }
    }
    Ok(())
}

pub async fn fork_session(id: Uuid) -> std::io::Result<Session> {
    let original = load_session(id).await;
    let session = match original {
        Some(s) => s,
        None => return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "session not found")),
    };
    let cwd = session.meta.cwd.clone();
    let model = session.meta.model.clone();
    let mut new_session = create_session(cwd, model).await;
    // 立刻落盘 meta + 空 jsonl
    let sessions_dir = paths::sessions_dir();
    let meta_path = sessions_dir.join(format!("{}.meta.json", new_session.meta.id));
    let jsonl_path = sessions_dir.join(format!("{}.jsonl", new_session.meta.id));
    tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&new_session.meta)?).await?;
    tokio::fs::write(&jsonl_path, b"").await?;
    // 复制消息
    replace_messages(new_session.meta.id, &session.messages).await?;
    new_session.messages = session.messages;
    Ok(new_session)
}

pub async fn delete_session(session_id: Uuid) -> std::io::Result<()> {
    let dir = paths::sessions_dir();
    for ext in &["meta.json", "jsonl", "todos.json"] {
        let p = dir.join(format!("{}.{}", session_id, ext));
        if p.exists() { tokio::fs::remove_file(&p).await?; }
    }
    Ok(())
}
