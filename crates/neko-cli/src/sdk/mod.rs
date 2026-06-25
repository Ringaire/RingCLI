//! SDK 模式：stdin/stdout NDJSON 双向通信。
//!
//! 协议：
//! ```jsonl
//! {"id":"<uuid>","type":"message","payload":"用户消息"}
//! {"id":"<uuid>","type":"ping"}
//! {"id":"<uuid>","type":"exit"}
//! ```
//!
//! 响应（每行一个 JSON，实时输出）：
//! ```jsonl
//! {"id":"<uuid>","type":"ready","payload":"..."}
//! {"id":"<uuid>","type":"text","payload":"增量文本"}
//! {"id":"<uuid>","type":"tool","payload":"工具调用摘要"}
//! {"id":"<uuid>","type":"done","payload":"完整文本"}
//! {"id":"<uuid>","type":"error","payload":"错误信息"}
//! {"id":"<uuid>","type":"pong"}
//! ```

use std::io::{self, BufRead, Write};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use neko_core::events::NekoEvent;

use crate::args::Args;
use crate::bootstrap;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkInput {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkOutput {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: String,
}

fn send(out: &SdkOutput) {
    let _ = writeln!(io::stdout(), "{}", serde_json::to_string(out).unwrap_or_default());
    let _ = io::stdout().flush();
}

/// Run SDK mode: read NDJSON from stdin, execute agent, stream events to stdout.
pub async fn run(args: &Args) -> Result<()> {
    let mut runtime = bootstrap::bootstrap(args, None).await?;

    let provider = runtime.provider.clone().ok_or_else(|| {
        anyhow::anyhow!("no provider configured")
    })?;

    // ready signal
    send(&SdkOutput {
        id: Uuid::nil(),
        msg_type: "ready".into(),
        payload: format!("neko SDK ready — model: {}", runtime.model),
    });

    let stdin = io::stdin();
    let reader = stdin.lock();
    let mut line = String::new();

    for raw in reader.lines() {
        line.clear();
        let text = match raw {
            Ok(t) if t.trim().is_empty() => continue,
            Ok(t) => t,
            Err(_) => break,
        };

        let input: SdkInput = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                send(&SdkOutput {
                    id: Uuid::nil(),
                    msg_type: "error".into(),
                    payload: format!("parse error: {e}"),
                });
                continue;
            }
        };

        match input.msg_type.as_str() {
            "ping" => {
                send(&SdkOutput {
                    id: input.id,
                    msg_type: "pong".into(),
                    payload: String::new(),
                });
            }
            "exit" | "stop" => break,
            "message" => {
                handle_message(&input, &mut runtime, &provider).await;
            }
            other => {
                send(&SdkOutput {
                    id: input.id,
                    msg_type: "error".into(),
                    payload: format!("unknown type: {other}"),
                });
            }
        }
    }

    Ok(())
}

async fn handle_message(
    input: &SdkInput,
    runtime: &mut crate::bootstrap::BootstrappedRuntime,
    provider: &std::sync::Arc<dyn neko_providers::provider::Provider>,
) {
    let prompt = &input.payload;

    // 构建 context
    let mut ctx = neko_engine::AgentContext::from_session(
        &runtime.session,
        runtime.model.clone(),
        Some(runtime.system_prompt.clone()),
    );
    ctx.add_message(neko_core::tools::Message::user_text(prompt));

    // 订阅事件总线
    let mut sub = runtime.bus.subscribe();

    // 构建 executor
    let executor = neko_engine::agent::orchestrator::build_executor(
        runtime.tools.clone(),
        runtime.permissions.clone(),
        runtime.bus.clone(),
        runtime.catalog.clone(),
        runtime.model.clone(),
        runtime.session.meta.id,
        runtime.cwd.clone(),
        neko_providers::provider::DEFAULT_MAX_OUTPUT_TOKENS as u64,
        provider.clone(),
        None,
    );

    let signal = CancellationToken::new();

    // 后台执行 + 实时输出事件
    let ctx_arc = std::sync::Arc::new(tokio::sync::Mutex::new(ctx));
    let ctx2 = ctx_arc.clone();
    let handle = tokio::spawn(async move {
        let mut guard = ctx2.lock().await;
        executor.run(&mut guard, signal).await
    });

    // 消费事件 → NDJSON 输出
    loop {
        match sub.recv().await {
            Ok(ev) => {
                let is_done = matches!(ev, NekoEvent::AgentDone { .. } | NekoEvent::AgentError { .. });
                let (msg_type, payload) = match &ev {
                    NekoEvent::AgentText { delta, .. } => ("text", delta.clone()),
                    NekoEvent::AgentTextDone { full, .. } => ("done", full.clone()),
                    NekoEvent::AgentToolCall { tool_name, input: tool_input, .. } => {
                        ("tool", format!("{tool_name}({tool_input})"))
                    }
                    NekoEvent::AgentError { error, .. } => ("error", error.clone()),
                    NekoEvent::AgentReasoning { delta, .. } => ("reasoning", delta.clone()),
                    _ => {
                        if is_done { break; }
                        continue;
                    }
                };
                send(&SdkOutput {
                    id: input.id,
                    msg_type: msg_type.into(),
                    payload,
                });
                if is_done { break; }
            }
            Err(_) => break,
        }
    }

    let _ = handle.await;
}
