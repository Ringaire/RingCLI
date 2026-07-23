//! --print 模式：一次性执行 + 输出格式化（text / json / stream-json）。
//!
//! - `ring -p "hello"` — 纯文本输出
//! - `ring -p "hello" --output-format json` — 完整 JSON 结果
//! - `ring -p "hello" --output-format stream-json` — NDJSON 流式事件

use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::info;

use ring_core::events::{EventBus, RingEvent};
use ring_engine::{AgentContext, AgentExecutor};

use crate::args::Args;
use crate::bootstrap;

pub async fn run(prompt: &str, args: &Args) -> Result<()> {
    let runtime = bootstrap::bootstrap(args, None).await?;

    let provider = runtime.provider.clone().ok_or_else(|| {
        anyhow::anyhow!("no provider configured — run `ring` and /connect first")
    })?;

    // 构建 context
    let mut ctx = AgentContext::from_session(
        &runtime.session,
        runtime.model.clone(),
        Some(runtime.system_prompt.clone()),
    );
    ctx.add_message(ring_core::tools::Message::user_text(prompt));

    // 持久化用户消息
    if let Some(user_msg) = ctx.messages.last().cloned() {
        ring_core::session::append_message(runtime.session.meta.id, user_msg)
            .await
            .ok();
    }

    // 构建 executor
    let executor = ring_engine::agent::orchestrator::build_executor(
        ring_engine::agent::orchestrator::ExecutorParams {
            tools: runtime.tools.clone(),
            permissions: runtime.permissions.clone(),
            bus: runtime.bus.clone(),
            catalog: runtime.catalog.clone(),
            current_model: runtime.model.clone(),
            session_id: runtime.session.meta.id,
            cwd: runtime.cwd.clone(),
            max_tokens: ring_providers::provider::DEFAULT_MAX_OUTPUT_TOKENS as u64,
            provider,
            perm_tx: None,
        }
    );

    let format = args.output_format.as_str();
    info!(format, "print mode");

    let ctx = Arc::new(TokioMutex::new(ctx));

    match format {
        "text" => run_text(executor, ctx, &runtime.bus).await,
        "json" => run_json(executor, ctx, &runtime.bus).await,
        "stream-json" => run_stream_json(executor, ctx, &runtime.bus).await,
        other => Err(anyhow::anyhow!("unknown output format: {other}")),
    }
}

// ── text：执行完输出纯文本 ────────────────────────────────────────────────────

async fn run_text(
    executor: AgentExecutor,
    ctx: Arc<TokioMutex<AgentContext>>,
    bus: &EventBus,
) -> Result<()> {
    let mut sub = bus.subscribe();
    let signal = CancellationToken::new();

    {
        let mut guard = ctx.lock().await;
        executor.run(&mut guard, signal).await;
    }

    // 从事件总线收集最终文本
    let mut text = String::new();
    while let Ok(ev) = sub.try_recv() {
        if let RingEvent::AgentTextDone { full, .. } = ev {
            text = full;
        }
    }

    if text.is_empty() {
        while let Ok(ev) = sub.try_recv() {
            if let RingEvent::AgentText { delta, .. } = ev {
                text.push_str(&delta);
            }
        }
    }

    print!("{}", text);
    if !text.ends_with('\n') {
        println!();
    }
    Ok(())
}

// ── json：执行完输出完整 JSON ──────────────────────────────────────────────────

#[derive(Serialize)]
struct JsonResult {
    role:       String,
    content:    String,
    tool_calls: Vec<JsonToolCall>,
    stop_reason: String,
}

#[derive(Serialize)]
struct JsonToolCall {
    name:  String,
    input: serde_json::Value,
}

async fn run_json(
    executor: AgentExecutor,
    ctx: Arc<TokioMutex<AgentContext>>,
    bus: &EventBus,
) -> Result<()> {
    let mut sub = bus.subscribe();
    let signal = CancellationToken::new();

    {
        let mut guard = ctx.lock().await;
        executor.run(&mut guard, signal).await;
    }

    // 收集结果
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut stop_reason = "end_turn".to_string();

    while let Ok(ev) = sub.try_recv() {
        match ev {
            RingEvent::AgentTextDone { full, .. } => text = full,
            RingEvent::AgentToolCall { tool_name, input, .. } => {
                tool_calls.push(JsonToolCall { name: tool_name, input });
            }
            RingEvent::AgentDone { .. } => { /* final */ }
            RingEvent::AgentError { error, .. } => {
                stop_reason = "error".to_string();
                text = error;
            }
            _ => {}
        }
    }

    if text.is_empty() {
        while let Ok(ev) = sub.try_recv() {
            if let RingEvent::AgentText { delta, .. } = ev {
                text.push_str(&delta);
            }
        }
    }

    let result = JsonResult {
        role: "assistant".into(),
        content: text,
        tool_calls,
        stop_reason,
    };

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

// ── stream-json：NDJSON 流式输出 ──────────────────────────────────────────────

async fn run_stream_json(
    executor: AgentExecutor,
    ctx: Arc<TokioMutex<AgentContext>>,
    bus: &EventBus,
) -> Result<()> {
    let mut sub = bus.subscribe();
    let signal = CancellationToken::new();

    // 后台执行 agent turn
    let ctx2 = ctx.clone();
    let handle = tokio::spawn(async move {
        let mut guard = ctx2.lock().await;
        executor.run(&mut guard, signal).await
    });

    // 实时输出事件
    while let Ok(ev) = sub.recv().await {
        let is_done = matches!(ev, RingEvent::AgentDone { .. } | RingEvent::AgentError { .. });
        println!("{}", serde_json::to_string(&ev)?);
        if is_done {
            break;
        }
    }

    let _ = handle.await;
    Ok(())
}
