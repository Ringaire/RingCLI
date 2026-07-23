pub mod compression;
pub mod db;
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

static PENDING: std::sync::LazyLock<Arc<Mutex<HashMap<Uuid, SessionMeta>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

// ── 公共 API ──────────────────────────────────────────────────────────────────

pub async fn init_dirs() -> std::io::Result<()> {
    // Migrate old XDG paths to new unified ~/.ring/ structure
    paths::migrate_from_xdg()?;

    // Create all required directories under ~/.ring/
    tokio::fs::create_dir_all(paths::sessions_dir()).await?;
    tokio::fs::create_dir_all(paths::config_dir()).await?;
    tokio::fs::create_dir_all(paths::cache_dir()).await?;
    tokio::fs::create_dir_all(paths::logs_dir()).await?;
    tokio::fs::create_dir_all(paths::skills_dir()).await?;

    // Initialize SQLite DB (synchronous, fast)
    tokio::task::spawn_blocking(|| {
        db::init();
        backfill_from_filesystem();
    })
    .await
    .map_err(|e| std::io::Error::other(format!("DB init task panicked: {e}")))?;

    Ok(())
}

/// 扫描 sessions 目录中的 .meta.json 文件，将缺失的导入 SQLite。
/// 幂等操作 — 已存在的记录会被 UPSERT 覆盖。
fn backfill_from_filesystem() {
    let dir = paths::sessions_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    let mut imported = 0u32;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // 只处理 .meta.json 文件
        if !name.ends_with(".meta.json") {
            continue;
        }

        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<SessionMeta>(&raw) else {
            continue;
        };

        db::upsert_thread(&meta);
        imported += 1;
    }

    if imported > 0 {
        tracing::debug!("backfill: imported {imported} session metas into SQLite");
    }
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
        let meta_path = sessions_dir.join(format!("{session_id}.meta.json"));
        let jsonl_path = sessions_dir.join(format!("{session_id}.jsonl"));
        tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;
        tokio::fs::write(&jsonl_path, b"").await?;
    }
    drop(pending);

    // 如果 JSONL 被压缩了，先解压
    tokio::task::spawn_blocking(move || compression::ensure_decompressed(session_id))
        .await
        .map_err(|e| std::io::Error::other(format!("decompress task panicked: {e}")))??;

    // 追加消息行
    let jsonl_path = sessions_dir.join(format!("{session_id}.jsonl"));
    let mut line = serde_json::to_string(&msg)?;
    line.push('\n');
    use tokio::io::AsyncWriteExt;
    let mut f = tokio::fs::OpenOptions::new()
        .create(true).append(true)
        .open(&jsonl_path).await?;
    f.write_all(line.as_bytes()).await?;

    // 更新 meta（read-modify-write on .meta.json）
    let meta_path = sessions_dir.join(format!("{session_id}.meta.json"));
    let updated_meta = if let Ok(raw) = tokio::fs::read_to_string(&meta_path).await {
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
            let to_write = serde_json::to_vec_pretty(&meta)?;
            tokio::fs::write(&meta_path, to_write).await?;
            Some(meta)
        } else {
            None
        }
    } else {
        None
    };

    // 同步更新 SQLite（非阻塞 — 失败不中断写入流程）
    if let Some(ref m) = updated_meta {
        let m = m.clone();
        tokio::task::spawn_blocking(move || {
            db::upsert_thread(&m);
        })
        .await
        .ok();
    }

    Ok(())
}

pub async fn list_sessions() -> Vec<SessionMeta> {
    // 先查 SQLite（快速路径）
    let db_result = tokio::task::spawn_blocking(|| db::list_threads(500))
        .await
        .unwrap_or_default();

    if !db_result.is_empty() {
        return db_result;
    }

    // Fallback: 遍历文件系统（兼容 DB 未初始化或损坏的情况）
    list_sessions_from_filesystem().await
}

async fn list_sessions_from_filesystem() -> Vec<SessionMeta> {
    let sessions_dir = paths::sessions_dir();
    let mut metas = Vec::new();

    if let Ok(mut rd) = tokio::fs::read_dir(&sessions_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let p = entry.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".meta.json") {
                if let Ok(raw) = tokio::fs::read_to_string(&p).await {
                    if let Ok(m) = serde_json::from_str::<SessionMeta>(&raw) {
                        metas.push(m);
                    }
                }
            }
        }
    }

    metas.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    metas
}

/// 按标题关键词搜索会话（SQLite LIKE 查询）。
pub async fn search_sessions(query: &str) -> Vec<SessionMeta> {
    let query = query.to_string();
    tokio::task::spawn_blocking(move || db::search_threads(&query))
        .await
        .unwrap_or_default()
}

pub async fn load_session(id: Uuid) -> Option<Session> {
    let sessions_dir = paths::sessions_dir();
    let meta_path = sessions_dir.join(format!("{id}.meta.json"));

    let meta_raw = tokio::fs::read_to_string(&meta_path).await.ok()?;
    let meta: SessionMeta = serde_json::from_str(&meta_raw).ok()?;

    // 透明读取 JSONL（自动处理 .zst 压缩）
    let jsonl_raw = compression::read_jsonl(id)
        .map_err(|e| tracing::warn!("failed to read JSONL for session {id}: {e}"))
        .unwrap_or_default();
    let messages = jsonl_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Message>(l).ok())
        .collect();

    Some(Session { meta, messages })
}

pub async fn replace_messages(session_id: Uuid, messages: &[Message]) -> std::io::Result<()> {
    // 如果文件被压缩了，先解压
    tokio::task::spawn_blocking(move || compression::ensure_decompressed(session_id))
        .await
        .map_err(|e| std::io::Error::other(format!("decompress task panicked: {e}")))??;

    let jsonl_path = paths::sessions_dir().join(format!("{session_id}.jsonl"));
    let mut content = String::new();
    for msg in messages {
        content.push_str(&serde_json::to_string(msg)?);
        content.push('\n');
    }
    tokio::fs::write(&jsonl_path, content.as_bytes()).await?;

    // 同步更新 SQLite message_count
    let count = messages.len();
    let session_id_clone = session_id;
    tokio::task::spawn_blocking(move || {
        if let Some(mut meta) = db::get_thread(session_id_clone) {
            meta.message_count = count;
            meta.updated_at = chrono::Utc::now().timestamp_millis();
            db::upsert_thread(&meta);
        }
    })
    .await
    .ok();

    Ok(())
}

pub async fn rename_session(session_id: Uuid, title: String) -> std::io::Result<()> {
    let meta_path = paths::sessions_dir().join(format!("{session_id}.meta.json"));
    if let Ok(raw) = tokio::fs::read_to_string(&meta_path).await {
        if let Ok(mut meta) = serde_json::from_str::<SessionMeta>(&raw) {
            meta.title = Some(title.clone());
            tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;

            // 同步 SQLite
            let session_id_clone = session_id;
            tokio::task::spawn_blocking(move || {
                if let Some(mut m) = db::get_thread(session_id_clone) {
                    m.title = Some(title);
                    db::upsert_thread(&m);
                }
            })
            .await
            .ok();
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

    // 修正 fork 后的 message_count（create_session 初始化为 0）
    new_session.meta.message_count = session.messages.len();
    new_session.meta.updated_at = chrono::Utc::now().timestamp_millis();

    tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&new_session.meta)?).await?;
    tokio::fs::write(&jsonl_path, b"").await?;

    // 复制消息
    replace_messages(new_session.meta.id, &session.messages).await?;
    new_session.messages = session.messages;
    Ok(new_session)
}

pub async fn delete_session(session_id: Uuid) -> std::io::Result<()> {
    let dir = paths::sessions_dir();

    // 删除文件（包括可能的 .zst 压缩文件）
    for ext in &["meta.json", "jsonl", "jsonl.zst", "todos.json"] {
        let p = dir.join(format!("{session_id}.{ext}"));
        if p.exists() {
            tokio::fs::remove_file(&p).await?;
        }
    }

    // 删除 SQLite 记录
    let id = session_id;
    tokio::task::spawn_blocking(move || {
        db::delete_thread(id);
    })
    .await
    .ok();

    Ok(())
}
