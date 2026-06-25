pub mod protocol;

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
use uuid::Uuid;

use protocol::*;

/// Connection state for the RCA client.
#[derive(Debug, Clone, PartialEq)]
pub enum RcaState {
    Disconnected,
    Connecting,
    Connected {
        session_id: Uuid,
        server_version: String,
        url: String,
    },
    Error(String),
}

/// Handles a single RCA connection lifecycle.
#[derive(Clone)]
pub struct RcaClient {
    pub state: Arc<Mutex<RcaState>>,
    cmd_tx: mpsc::UnboundedSender<RcaCommand>,
    auth_token: Arc<Mutex<Option<String>>>,
}

enum RcaCommand {
    Connect { url: String, client_id: String, auth_token: Option<String> },
    Disconnect,
    SendMessage(String),
}

impl RcaClient {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<String>) {
        let state = Arc::new(Mutex::new(RcaState::Disconnected));
        let auth_token = Arc::new(Mutex::new(None));
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();

        let client = Self { state, cmd_tx, auth_token };

        // Spawn background connection manager
        let state_clone = client.state.clone();
        tokio::spawn(rca_loop(state_clone, cmd_rx, msg_tx));

        (client, msg_rx)
    }

    pub async fn connect(&self, url: &str, client_id: &str, token: Option<String>) {
        *self.auth_token.lock().await = token.clone();
        let _ = self.cmd_tx.send(RcaCommand::Connect {
            url: url.to_string(),
            client_id: client_id.to_string(),
            auth_token: token,
        });
    }

    pub async fn disconnect(&self) {
        let _ = self.cmd_tx.send(RcaCommand::Disconnect);
    }

    pub async fn rca_state(&self) -> RcaState {
        self.state.lock().await.clone()
    }

    /// Check if this client has a stored auth token.
    /// Send a TaskResult back to NekoRCA.
    pub fn send_result(&self, result: TaskResult) {
        let env = build_envelope(
            MsgType::TaskResult,
            Direction::Upstream,
            serde_json::to_value(result).unwrap(),
        );
        let text = serde_json::to_string(&env).unwrap_or_default();
        let _ = self.cmd_tx.send(RcaCommand::SendMessage(text));
    }

    pub async fn has_token(&self) -> bool {
        self.auth_token.lock().await.is_some()
    }

    pub async fn set_auth_token(&self, token: Option<String>) {
        *self.auth_token.lock().await = token;
    }
}

/// Background task: manages WS connection, reconnect, heartbeat.
async fn rca_loop(
    state: Arc<Mutex<RcaState>>,
    mut cmd_rx: mpsc::UnboundedReceiver<RcaCommand>,
    msg_tx: mpsc::UnboundedSender<String>,
) {
    let mut ws: Option<(
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Uuid,
    )> = None;
    let mut heartbeat_seq = 0u64;

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(RcaCommand::Connect { url, client_id, auth_token }) => {
                        // Close existing connection
                        if let Some((mut ws_stream, _)) = ws.take() {
                            let _ = ws_stream.close(None).await;
                        }
                        *state.lock().await = RcaState::Connecting;

                        match connect_rca(&url, &client_id, auth_token).await {
                            Ok((ws_stream, ack)) => {
                                info!("RCA connected: session={}", ack.session_id);
                                *state.lock().await = RcaState::Connected {
                                    session_id: ack.session_id,
                                    server_version: ack.server_version,
                                    url: url.clone(),
                                };
                                ws = Some((ws_stream, ack.session_id));
                            }
                            Err(e) => {
                                error!("RCA connect failed: {e}");
                                *state.lock().await = RcaState::Error(e);
                            }
                        }
                    }
                    Some(RcaCommand::SendMessage(text)) => {
                        if let Some((ref mut ws_stream, _)) = ws {
                            let _ = ws_stream.send(Message::Text(text.into())).await;
                        }
                    }
                    Some(RcaCommand::Disconnect) | None => {
                        if let Some((mut ws_stream, session_id)) = ws.take() {
                            info!("RCA disconnect: {session_id}");
                            let _ = ws_stream.close(None).await;
                        }
                        *state.lock().await = RcaState::Disconnected;
                        if cmd.is_none() {
                            break; // channel closed, exit loop
                        }
                    }
                }
            }
            msg = async {
                if let Some((ref mut ws_stream, _)) = ws {
                    ws_stream.next().await
                } else {
                    futures_util::future::pending().await
                }
            } => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(env) = serde_json::from_str::<Envelope>(&text) {
                            match env.msg_type {
                                MsgType::Heartbeat => {
                                    let pong = build_envelope(MsgType::HeartbeatAck, Direction::Upstream, env.payload);
                                    if let Some((ref mut ws_stream, _)) = ws {
                                        let _ = ws_stream.send(Message::Text(serde_json::to_string(&pong).unwrap().into())).await;
                                    }
                                }
                                MsgType::HeartbeatAck => {
                                    // heartbeat acknowledged
                                }
                                MsgType::AssignTask => {
                                    // Forward to main event loop
                                    let _ = msg_tx.send(text.to_string());
                                }
                                MsgType::RegisterAck => {
                                    // Already handled during connect
                                }
                                MsgType::Error => {
                                    if let Ok(err) = serde_json::from_value::<Error>(env.payload) {
                                        error!("RCA error: {} ({})", err.message, err.code);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        if let Some((_, session_id)) = ws.take() {
                            warn!("RCA disconnected: {session_id}");
                            *state.lock().await = RcaState::Disconnected;
                        }
                    }
                    Some(Err(e)) => {
                        error!("RCA WS error: {e}");
                        if let Some((_, session_id)) = ws.take() {
                            *state.lock().await = RcaState::Error(e.to_string());
                        }
                    }
                    _ => {}
                }
            }
            // Periodic heartbeat
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                if ws.is_some() {
                    heartbeat_seq += 1;
                    let hb = build_envelope(
                        MsgType::Heartbeat,
                        Direction::Upstream,
                        serde_json::to_value(Heartbeat { seq: heartbeat_seq }).unwrap(),
                    );
                    if let Some((ref mut ws_stream, _)) = ws {
                        let text = serde_json::to_string(&hb).unwrap();
                        let _ = ws_stream.send(Message::Text(text.into())).await;
                    }
                }
            }
        }
    }
}

/// Connect to RCA, send Register, wait for RegisterAck.
async fn connect_rca(
    url: &str,
    client_id: &str,
    auth_token: Option<String>,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        RegisterAck,
    ),
    String,
> {
    let (ws_stream, _) = connect_async(url)
        .await
        .map_err(|e| format!("WS connect failed: {e}"))?;

    let register = Envelope {
        id: Uuid::new_v4(),
        msg_type: MsgType::Register,
        payload: serde_json::to_value(Register {
            client_id: client_id.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: vec![
                "chat".to_string(),
                "tools".to_string(),
                "tasks".to_string(),
            ],
            labels: Some(vec!["nekocli".to_string()]),
            auth_token,
        })
        .unwrap(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        direction: Direction::Upstream,
    };

    // We need a split stream - use a tuple to hold the write half
    let (mut write, mut read) = ws_stream.split();

    // Send register
    let reg_text = serde_json::to_string(&register).unwrap();
    write
        .send(Message::Text(reg_text.into()))
        .await
        .map_err(|e| format!("send register failed: {e}"))?;

    // Wait for RegisterAck
    loop {
        match read.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(env) = serde_json::from_str::<Envelope>(&text) {
                    if env.msg_type == MsgType::RegisterAck {
                        let ack: RegisterAck = serde_json::from_value(env.payload)
                            .map_err(|e| format!("bad ack payload: {e}"))?;
                        // Reassemble the stream
                        let ws_stream = write.reunite(read)
                            .map_err(|_| "failed to reunite WS stream".to_string())?;
                        return Ok((ws_stream, ack));
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err("connection closed before register ack".to_string());
            }
            Some(Err(e)) => return Err(format!("WS error during register: {e}")),
            _ => continue,
        }
    }
}

fn build_envelope(msg_type: MsgType, direction: Direction, payload: serde_json::Value) -> Envelope {
    Envelope {
        id: Uuid::new_v4(),
        msg_type,
        payload,
        timestamp: chrono::Utc::now().timestamp_millis(),
        direction,
    }
}

// ── CLI 入口：neko --rca <url> ────────────────────────────────────────────────

/// 连接到 RCA hub，注册为远程 worker，接收并执行任务。
pub async fn connect_and_run(
    hub_url: &str,
    runtime: &mut crate::bootstrap::BootstrappedRuntime,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;

    info!(url = hub_url, "RCA connecting to hub");

    let (ws, _) = connect_async(hub_url).await?;
    let (mut tx, mut rx) = ws.split();

    // 注册
    let client_id = format!("neko-{}", &Uuid::new_v4().to_string()[..8]);
    let reg = build_envelope(
        MsgType::Register,
        Direction::Upstream,
        serde_json::to_value(Register {
            client_id: client_id.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec!["chat".into(), "code".into()],
            labels: None,
            auth_token: None,
        })?,
    );
    tx.send(Message::Text(serde_json::to_string(&reg)?)).await?;

    // 等待 RegisterAck
    let mut heartbeat_secs = 30u64;
    let mut registered = false;
    while let Some(msg) = rx.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(env) = serde_json::from_str::<Envelope>(&text) {
                    if env.msg_type == MsgType::RegisterAck {
                        let ack: RegisterAck = serde_json::from_value(env.payload).unwrap_or(RegisterAck {
                            session_id: Uuid::new_v4(),
                            heartbeat_interval_secs: 30,
                            server_version: "unknown".into(),
                        });
                        heartbeat_secs = ack.heartbeat_interval_secs.max(5);
                        info!(session_id = %ack.session_id, "RCA registered");
                        registered = true;
                        break;
                    }
                }
            }
            _ => break,
        }
    }

    if !registered {
        anyhow::bail!("RCA registration failed");
    }

    // 心跳 task
    let (hb_tx, mut hb_rx) = mpsc::unbounded_channel::<Message>();
    {
        let hb_tx = hb_tx.clone();
        tokio::spawn(async move {
            let mut seq = 0u64;
            loop {
                tokio::time::sleep(Duration::from_secs(heartbeat_secs)).await;
                seq += 1;
                let hb = build_envelope(
                    MsgType::Heartbeat,
                    Direction::Upstream,
                    serde_json::to_value(Heartbeat { seq }).unwrap(),
                );
                let text = serde_json::to_string(&hb).unwrap_or_default();
                if hb_tx.send(Message::Text(text.into())).is_err() {
                    break;
                }
            }
        });
    }

    // 主循环
    info!("RCA worker ready — waiting for tasks");
    loop {
        tokio::select! {
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(env) = serde_json::from_str::<Envelope>(&text) {
                            match env.msg_type {
                                MsgType::AssignTask => {
                                    let task: AssignTask = serde_json::from_value(env.payload)
                                        .unwrap_or(AssignTask {
                                            task_id: Uuid::new_v4(),
                                            platform: "unknown".into(),
                                            platform_user_id: String::new(),
                                            conversation_id: String::new(),
                                            message: TaskMessage { text: String::new(), attachments: None },
                                            context: None,
                                        });
                                    info!(task_id = %task.task_id, "received task");

                                    let result = execute_task(&task, runtime).await;
                                    let (status, output, error) = match result {
                                        Ok(text) => (TaskStatus::Completed, Some(TaskOutput { text, actions: None, metadata: None }), None),
                                        Err(e) => (TaskStatus::Failed, None, Some(e.to_string())),
                                    };
                                    let tr = TaskResult {
                                        task_id: task.task_id,
                                        status,
                                        output,
                                        error,
                                    };
                                    let resp = build_envelope(
                                        MsgType::TaskResult,
                                        Direction::Upstream,
                                        serde_json::to_value(tr)?,
                                    );
                                    tx.send(Message::Text(serde_json::to_string(&resp)?.into())).await?;
                                }
                                MsgType::HeartbeatAck => {}
                                MsgType::Error => {
                                    if let Ok(e) = serde_json::from_value::<Error>(env.payload) {
                                        warn!(code = e.code, msg = e.message, "hub error");
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("RCA connection closed");
                        break;
                    }
                    _ => {}
                }
            }
            hb = hb_rx.recv() => {
                if let Some(msg) = hb {
                    let _ = tx.send(msg).await;
                }
            }
        }
    }

    Ok(())
}

/// 执行一个任务（prompt → agent turn → 返回文本结果）。
async fn execute_task(
    task: &AssignTask,
    runtime: &mut crate::bootstrap::BootstrappedRuntime,
) -> anyhow::Result<String> {
    let provider = runtime.provider.clone()
        .ok_or_else(|| anyhow::anyhow!("no provider"))?;

    let mut ctx = neko_engine::AgentContext::from_session(
        &runtime.session,
        runtime.model.clone(),
        Some(runtime.system_prompt.clone()),
    );
    ctx.add_message(neko_core::tools::Message::user_text(&task.message.text));

    let mut sub = runtime.bus.subscribe();

    let executor = neko_engine::agent::orchestrator::build_executor(
        runtime.tools.clone(),
        runtime.permissions.clone(),
        runtime.bus.clone(),
        runtime.catalog.clone(),
        runtime.model.clone(),
        runtime.session.meta.id,
        runtime.cwd.clone(),
        neko_providers::provider::DEFAULT_MAX_OUTPUT_TOKENS as u64,
        provider,
        None,
    );

    executor.run(&mut ctx, tokio_util::sync::CancellationToken::new()).await;

    let mut text = String::new();
    while let Ok(ev) = sub.try_recv() {
        if let neko_core::events::NekoEvent::AgentTextDone { full, .. } = ev {
            text = full;
        }
    }

    Ok(text)
}

/// Send a TaskResult back to RCA.
pub async fn send_task_result(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, result: TaskResult) -> Result<(), String> {
    let env = build_envelope(
        MsgType::TaskResult,
        Direction::Upstream,
        serde_json::to_value(result).unwrap(),
    );
    let text = serde_json::to_string(&env).map_err(|e| format!("serialize failed: {e}"))?;
    ws.send(Message::Text(text.into()))
        .await
        .map_err(|e| format!("send failed: {e}"))
}
