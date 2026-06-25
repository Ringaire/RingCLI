//! HTTP server 模式：`neko --serve 127.0.0.1:8765`
//!
//! REST API：
//! - `GET  /health` — 健康检查
//! - `POST /v1/chat` — 一次性对话 `{"prompt": "...", "model": "..."}`
//! - `GET  /v1/sessions` — 列出会话
//! - `GET  /v1/sessions/{id}` — 获取会话消息

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use neko_core::events::NekoEvent;
use neko_engine::AgentContext;

use crate::args::Args;
use crate::bootstrap;

// ── 启动 ──────────────────────────────────────────────────────────────────────

pub async fn serve(addr: &str) -> anyhow::Result<()> {
    // bootstrap runtime
    let args = default_server_args();
    let runtime = bootstrap::bootstrap(&args, None).await?;

    let state = Arc::new(ServerState {
        provider:      runtime.provider.clone(),
        tools:         runtime.tools.clone(),
        permissions:   runtime.permissions.clone(),
        bus:           runtime.bus.clone(),
        catalog:       runtime.catalog.clone(),
        model:         runtime.model.clone(),
        system_prompt: runtime.system_prompt.clone(),
        cwd:           runtime.cwd.clone(),
        session:       runtime.session.clone(),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/chat", post(chat))
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/{id}", get(get_session))
        .with_state(state);

    info!("neko server on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn default_server_args() -> Args {
    Args {
        prompt: None,
        mode: "build".into(),
        print: false,
        output_format: "text".into(),
        r#continue: false,
        resume: None,
        list_sessions: false,
        model: None,
        provider: None,
        cwd: None,
        dangerously_skip_permissions: true,
        extended_thinking: false,
        verbose: false,
        debug: None,
        add_dir: Vec::new(),
        no_tui: true,
        serve: None,
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

struct ServerState {
    provider:      Option<Arc<dyn neko_providers::provider::Provider>>,
    tools:         Arc<dyn neko_core::tools::ToolRegistry>,
    permissions:   Arc<tokio::sync::Mutex<neko_core::permissions::DefaultPermissionEngine>>,
    bus:           neko_core::events::EventBus,
    catalog:       Vec<neko_core::agent::ModelCatalogEntry>,
    model:         String,
    system_prompt: String,
    cwd:           std::path::PathBuf,
    session:       neko_core::session::Session,
}

// ── 请求 / 响应 结构 ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatRequest {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    role:        String,
    content:     String,
    stop_reason: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn chat(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let provider = state.provider.clone().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, "no provider configured".into())
    })?;

    // 每次请求新建 context（无状态）
    let mut ctx = AgentContext::from_session(
        &state.session,
        state.model.clone(),
        Some(state.system_prompt.clone()),
    );
    ctx.add_message(neko_core::tools::Message::user_text(&req.prompt));

    // 订阅事件总线收集输出
    let mut sub = state.bus.subscribe();

    // 构建 executor
    let executor = neko_engine::agent::orchestrator::build_executor(
        state.tools.clone(),
        state.permissions.clone(),
        state.bus.clone(),
        state.catalog.clone(),
        state.model.clone(),
        state.session.meta.id,
        state.cwd.clone(),
        neko_providers::provider::DEFAULT_MAX_OUTPUT_TOKENS as u64,
        provider,
        None,
    );

    let signal = tokio_util::sync::CancellationToken::new();

    // 执行
    executor.run(&mut ctx, signal).await;

    // 收集结果
    let mut text = String::new();
    let mut stop_reason = "end_turn".to_string();

    while let Ok(ev) = sub.try_recv() {
        match ev {
            NekoEvent::AgentTextDone { full, .. } => text = full,
            NekoEvent::AgentText { delta, .. } => {
                if text.is_empty() { text.push_str(&delta); }
            }
            NekoEvent::AgentError { error, .. } => {
                stop_reason = "error".to_string();
                text = error;
            }
            _ => {}
        }
    }

    Ok(Json(ChatResponse {
        role: "assistant".into(),
        content: text,
        stop_reason,
    }))
}

async fn list_sessions() -> impl IntoResponse {
    let sessions = neko_core::session::list_sessions().await;
    let entries: Vec<serde_json::Value> = sessions
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "title": s.title,
                "message_count": s.message_count,
                "updated_at": s.updated_at,
            })
        })
        .collect();
    Json(serde_json::json!({ "sessions": entries }))
}

async fn get_session(Path(id): Path<uuid::Uuid>) -> axum::response::Response {
    match neko_core::session::load_session(id).await {
        Some(session) => {
            let messages: Vec<serde_json::Value> = session
                .messages
                .into_iter()
                .map(|m| {
                    let role = match m.role {
                        neko_core::tools::MessageRole::User => "user",
                        neko_core::tools::MessageRole::Assistant => "assistant",
                        neko_core::tools::MessageRole::ToolResult => "tool",
                    };
                    let text: String = m
                        .content
                        .iter()
                        .filter_map(|b| {
                            if let neko_core::tools::ContentBlock::Text { text } = b {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    serde_json::json!({ "role": role, "content": text })
                })
                .collect();
            Json(serde_json::json!({
                "session_id": session.meta.id,
                "title": session.meta.title,
                "messages": messages,
            })).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "session not found" })),
        )
            .into_response(),
    }
}
