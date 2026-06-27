//! HTTP server 模式：`neko --serve 127.0.0.1:8765`
//!
//! REST API：
//! - `GET  /health` — 健康检查
//! - `POST /v1/chat` — 一次性对话 `{"prompt": "...", "model": "..."}`
//! - `GET  /v1/sessions` — 列出会话
//! - `GET  /v1/sessions/{id}` — 获取会话消息

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt as _};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

use neko_core::events::NekoEvent;
use neko_engine::AgentContext;

use crate::args::Args;
use crate::bootstrap;

// ── 启动 ──────────────────────────────────────────────────────────────────────

pub async fn serve(addr: &str) -> anyhow::Result<()> {
    // 智能解析地址：8765 → 127.0.0.1:8765，:8765 → 0.0.0.0:8765
    let bind_addr = if addr.contains(':') {
        addr.to_string()
    } else if let Ok(port) = addr.parse::<u16>() {
        format!("127.0.0.1:{port}")
    } else {
        addr.to_string()
    };
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
        .route("/v1/models", get(list_models))
        .route("/v1/providers", get(list_providers))
        .route("/v1/chat", post(chat))
        .route("/v1/chat/stream", post(chat_stream))
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/{id}", get(get_session))
        // allow browser apps (e.g. NekoApp) to call this server directly
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
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
        rca: None,
        sdk: false,
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
    reasoning:   Option<String>,
    stop_reason: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn list_models(
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    let models: Vec<serde_json::Value> = state
        .catalog
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "role": m.role.as_str(),
            })
        })
        .collect();
    Json(serde_json::json!({ "object": "list", "models": models }))
}

async fn list_providers(
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    let active = state.provider.as_ref().map(|p| {
        serde_json::json!({
            "id": p.id(),
            "name": p.display_name(),
            "default_model": p.default_model(),
            "active_model": state.model,
        })
    });
    Json(serde_json::json!({ "active": active }))
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

    // 并发收集事件：用 recv().await 可靠抓全量，避免 try_recv 漏事件。
    // 共享收集状态，collector task 持续消费直到 AgentDone / AgentError。
    let text_out = Arc::new(Mutex::new(String::new()));
    let reasoning_out = Arc::new(Mutex::new(String::new()));
    let stop_out = Arc::new(Mutex::new("end_turn".to_string()));

    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<()>();
    let text_c = text_out.clone();
    let reasoning_c = reasoning_out.clone();
    let stop_c = stop_out.clone();

    let collector = tokio::spawn(async move {
        loop {
            match sub.recv().await {
                Ok(ev) => match ev {
                    NekoEvent::AgentTextDone { full, .. } => {
                        *text_c.lock().unwrap() = full;
                    }
                    NekoEvent::AgentText { delta, .. } => {
                        // 累加所有 delta（不再只取第一个）
                        text_c.lock().unwrap().push_str(&delta);
                    }
                    NekoEvent::AgentReasoningDone { full, .. } => {
                        *reasoning_c.lock().unwrap() = full;
                    }
                    NekoEvent::AgentReasoning { delta, .. } => {
                        reasoning_c.lock().unwrap().push_str(&delta);
                    }
                    NekoEvent::AgentDone { stop_reason, .. } => {
                        *stop_c.lock().unwrap() = stop_reason;
                        break;
                    }
                    NekoEvent::AgentError { error, .. } => {
                        *stop_c.lock().unwrap() = "error".to_string();
                        *text_c.lock().unwrap() = error;
                        break;
                    }
                    _ => {}
                },
                Err(_) => break,
            }
        }
        let _ = done_tx.send(());
    });

    // 执行 agent turn
    executor.run(&mut ctx, signal).await;

    // 等 collector 收完（AgentDone 已发，最多再等残余事件）
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut done_rx).await;
    collector.abort();

    let content = text_out.lock().unwrap().clone();
    let reasoning = {
        let r = reasoning_out.lock().unwrap().clone();
        if r.is_empty() { None } else { Some(r) }
    };
    let stop_reason = stop_out.lock().unwrap().clone();

    Ok(Json(ChatResponse {
        role: "assistant".into(),
        content,
        reasoning,
        stop_reason,
    }))
}

/// 流式对话：`POST /v1/chat/stream` — NDJSON 行流（text-reasoning-done）
async fn chat_stream(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ChatRequest>,
) -> Result<axum::response::Response<Body>, (StatusCode, String)> {
    let provider = state.provider.clone().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, "no provider configured".into())
    })?;

    let model = req.model.unwrap_or_else(|| state.model.clone());
    let mut ctx = AgentContext::from_session(
        &state.session,
        model.clone(),
        Some(state.system_prompt.clone()),
    );
    ctx.add_message(neko_core::tools::Message::user_text(&req.prompt));

    let mut sub = state.bus.subscribe();
    let session_id = state.session.meta.id;

    let executor = neko_engine::agent::orchestrator::build_executor(
        state.tools.clone(),
        state.permissions.clone(),
        state.bus.clone(),
        state.catalog.clone(),
        model,
        session_id,
        state.cwd.clone(),
        neko_providers::provider::DEFAULT_MAX_OUTPUT_TOKENS as u64,
        provider,
        None,
    );

    let signal = tokio_util::sync::CancellationToken::new();

    // agent turn 在后台跑；forwarder 把事件转成 NDJSON 行，done/error 时关闭通道结束流
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tx_f = tx.clone();
    tokio::spawn(async move {
        executor.run(&mut ctx, signal).await;
    });

    tokio::spawn(async move {
        loop {
            match sub.recv().await {
                Ok(ev) => {
                    if ev.session_id() != session_id {
                        continue;
                    }
                    let line = match &ev {
                        NekoEvent::AgentReasoning { delta, .. } => {
                            Some(format!("{}\n", serde_json::json!({ "type": "reasoning", "delta": delta })))
                        }
                        NekoEvent::AgentReasoningDone { full, .. } => {
                            Some(format!("{}\n", serde_json::json!({ "type": "reasoning_done", "full": full })))
                        }
                        NekoEvent::AgentText { delta, .. } => {
                            Some(format!("{}\n", serde_json::json!({ "type": "text", "delta": delta })))
                        }
                        NekoEvent::AgentTextDone { full, .. } => {
                            Some(format!("{}\n", serde_json::json!({ "type": "text_done", "full": full })))
                        }
                        NekoEvent::AgentToolCall { call_id, tool_name, input, .. } |
                        NekoEvent::ToolStart { call_id, tool_name, input, .. } => {
                            Some(format!("{}\n", serde_json::json!({ "type": "tool_start", "call_id": call_id, "tool": tool_name, "input": input })))
                        }
                        NekoEvent::ToolEnd { call_id, tool_name, ok, duration_ms, .. } => {
                            Some(format!("{}\n", serde_json::json!({ "type": "tool_end", "call_id": call_id, "tool": tool_name, "ok": ok, "duration_ms": duration_ms })))
                        }
                        NekoEvent::AgentDone { stop_reason, .. } => {
                            let l = format!("{}\n", serde_json::json!({ "type": "done", "stop_reason": stop_reason }));
                            let _ = tx_f.send(l);
                            break;
                        }
                        NekoEvent::AgentError { error, .. } => {
                            let l = format!("{}\n", serde_json::json!({ "type": "error", "error": error }));
                            let _ = tx_f.send(l);
                            break;
                        }
                        _ => None,
                    };
                    if let Some(l) = line {
                        if tx_f.send(l).is_err() {
                            break; // client disconnected
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let stream = UnboundedReceiverStream::new(rx)
        .map(Ok::<_, std::io::Error>);

    Ok(axum::response::Response::builder()
        .header("content-type", "application/x-ndjson; charset=utf-8")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap())
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
