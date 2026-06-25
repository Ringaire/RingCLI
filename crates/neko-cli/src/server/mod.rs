use std::sync::Arc;

use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json,
};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Start the HTTP+WS server.
pub async fn serve(host: &str, port: u16, _auth_token: Option<String>) -> anyhow::Result<()> {
    let state = Arc::new(ServerState::new(_auth_token));

    let app = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({"status":"ok"})) }))
        .route("/v1/sessions", get(list_sessions).post(create_session))
        .route("/v1/sessions/{id}/ws", get(session_ws))
        .with_state(state);

    let addr = format!("{host}:{port}");
    info!("Neko server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ── State ──

struct ServerState {
    auth_token: Option<String>,
    sessions:   tokio::sync::RwLock<std::collections::HashMap<Uuid, SessionState>>,
}

struct SessionState {
    created_at: i64,
    tx: mpsc::UnboundedSender<String>,
}

impl ServerState {
    fn new(auth_token: Option<String>) -> Self {
        Self {
            auth_token,
            sessions: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

// ── Handlers ──

/// GET /v1/sessions — list active sessions.
async fn list_sessions(
    axum::extract::State(state): axum::extract::State<Arc<ServerState>>,
) -> impl IntoResponse {
    let sessions = state.sessions.read().await;
    let ids: Vec<Uuid> = sessions.keys().copied().collect();
    Json(serde_json::json!({ "sessions": ids }))
}

/// POST /v1/sessions — create a new session.
async fn create_session(
    axum::extract::State(state): axum::extract::State<Arc<ServerState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let session_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::unbounded_channel();

    let sess = SessionState {
        created_at: chrono::Utc::now().timestamp_millis(),
        tx,
    };
    state.sessions.write().await.insert(session_id, sess);

    info!("session created: {session_id}");
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "session_id": session_id,
            "ws_url": format!("/v1/sessions/{session_id}/ws"),
        })),
    )
}

/// GET /v1/sessions/{id}/ws — WebSocket for a session.
async fn session_ws(
    axum::extract::State(state): axum::extract::State<Arc<ServerState>>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_session_ws(socket, state, id))
}

/// WebSocket handler: relay messages to/from the agent.
async fn handle_session_ws(mut ws: WebSocket, _state: Arc<ServerState>, session_id: Uuid) {
    info!("session WS connected: {session_id}");

    loop {
        match ws.recv().await {
            Some(Ok(Message::Text(text))) => {
                // TODO: forward to agent, get response
                let reply = serde_json::json!({
                    "id": Uuid::new_v4(),
                    "type": "echo",
                    "payload": text.to_string(),
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                });
                let _ = ws.send(Message::Text(serde_json::to_string(&reply).unwrap().into())).await;
            }
            Some(Ok(Message::Close(_))) | None => break,
            Some(Err(e)) => {
                error!("WS error {session_id}: {e}");
                break;
            }
            _ => {}
        }
    }

    info!("session WS closed: {session_id}");
    _state.sessions.write().await.remove(&session_id);
}
