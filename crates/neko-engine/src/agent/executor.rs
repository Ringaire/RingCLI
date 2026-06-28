use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::debug;
use uuid::Uuid;

use neko_core::events::{BashStream, EventBus, NekoEvent};
use neko_core::permissions::{AccessCheck, DefaultPermissionEngine, PermissionAction};
use neko_core::session;
use neko_core::tools::{ContentBlock, Message, MessageRole, ToolContext, ToolRegistry};
use neko_providers::provider::{
    ChatRequest, Provider, StopReason, StreamChunk, StreamEvent, DEFAULT_MAX_OUTPUT_TOKENS,
};

use super::context::AgentContext;
use super::permission::{PermissionDecision, PermissionRequest, PermissionSender};
use super::tool_preview::extract_tool_preview;
use super::turn::TurnResult;

const MAX_TURNS: usize = 20;

const READ_ONLY_TOOLS: &[&str] = &[
    "read_file", "glob", "grep", "tree",
    "web_fetch", "web_search",
    "list_sessions", "token_count",
    "lsp_diagnostics", "lsp_refs",
];

fn is_read_only(tool_name: &str) -> bool {
    READ_ONLY_TOOLS.contains(&tool_name)
}

pub struct AgentExecutor {
    pub provider:    Arc<dyn Provider>,
    pub tools:       Arc<dyn ToolRegistry>,
    pub permissions: Arc<Mutex<DefaultPermissionEngine>>,
    pub bus:         EventBus,
    pub session_id:  Uuid,
    pub cwd:         std::path::PathBuf,
    /// 权限请求通道；None 时 Ask 默认拒绝（安全默认）。
    pub permission_tx: Option<PermissionSender>,
    /// 本 executor 所属子 agent；None 表示主 agent。
    /// 所有 emit 的事件都打上此 id，供前端区分主/次。
    pub sub_agent_id: Option<Uuid>,
    /// 最大 agent 轮数（spawn 出的子 agent 可自定义）。
    pub max_turns: usize,
    /// 是否把消息持久化到磁盘（子 agent 通常为 false）。
    pub persist: bool,
    /// Extended thinking 预算（token 数）；None 表示关闭。
    pub thinking_budget: Option<u32>,
    /// 单次请求最大输出 token 数（来自 SessionConfig）。
    pub max_output_tokens: u32,
    /// reasoning effort 级别（low/medium/high/max）。
    pub reasoning_effort: Option<String>,
    /// 后台子 Agent 完成结果池（task_id → output）。
    pub bg_results: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<uuid::Uuid, String>>>,
}

impl AgentExecutor {
    /// 主 agent executor（持久化、无子 id、默认轮数）。
    pub fn main(
        provider:      Arc<dyn Provider>,
        tools:         Arc<dyn ToolRegistry>,
        permissions:   Arc<Mutex<DefaultPermissionEngine>>,
        bus:           EventBus,
        session_id:    Uuid,
        cwd:           std::path::PathBuf,
        permission_tx: Option<PermissionSender>,
    ) -> Self {
        Self {
            provider,
            tools,
            permissions,
            bus,
            session_id,
            cwd,
            permission_tx,
            sub_agent_id:       None,
            max_turns:          MAX_TURNS,
            persist:            true,
            thinking_budget:    None,
            max_output_tokens:  DEFAULT_MAX_OUTPUT_TOKENS,
            reasoning_effort:   None,
            bg_results: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// 子 agent executor（不持久化、带子 id、自定义轮数）。
    #[allow(clippy::too_many_arguments)]
    pub fn sub(
        provider:      Arc<dyn Provider>,
        tools:         Arc<dyn ToolRegistry>,
        permissions:   Arc<Mutex<DefaultPermissionEngine>>,
        bus:           EventBus,
        session_id:    Uuid,
        cwd:           std::path::PathBuf,
        permission_tx: Option<PermissionSender>,
        sub_agent_id:  Uuid,
        max_turns:     usize,
    ) -> Self {
        Self {
            provider,
            tools,
            permissions,
            bus,
            session_id,
            cwd,
            permission_tx,
            sub_agent_id:       Some(sub_agent_id),
            max_turns,
            persist:            false,
            thinking_budget:    None,
            max_output_tokens:  DEFAULT_MAX_OUTPUT_TOKENS,
            reasoning_effort:   None,
            bg_results: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub async fn run(
        &self,
        ctx:    &mut AgentContext,
        signal: CancellationToken,
    ) -> TurnResult {
        for turn in 0..self.max_turns {
            debug!(turn, sub_agent = ?self.sub_agent_id, "agent turn start");

            // 每轮开头：检查后台子 Agent 完成结果，注入上下文
            let completed: Vec<(uuid::Uuid, String)> = {
                let mut pool = self.bg_results.lock().await;
                pool.drain().collect()
            };
            for (task_id, output) in completed {
                let msg = format!(
                    "[Background sub-agent {} completed]\n\n{}",
                    &task_id.to_string()[..8],
                    output,
                );
                ctx.add_message(Message::user_text(&msg));
                debug!(%task_id, "injected bg sub-agent result");
            }

            let result = self.do_turn(ctx, signal.clone()).await;
            match result {
                TurnResult::Continue => continue,
                other => return other,
            }
        }
        TurnResult::MaxTurns
    }

    async fn do_turn(&self, ctx: &mut AgentContext, signal: CancellationToken) -> TurnResult {
        self.bus.emit(NekoEvent::AgentThinking {
            session_id:   self.session_id,
            sub_agent_id: self.sub_agent_id,
        });

        // prune：截断老的 tool_result，省 token（只影响发给 LLM 的消息，不改原始序列）
        let mut messages_for_llm = ctx.messages.clone();
        prune_tool_results(&mut messages_for_llm);

        let req = ChatRequest {
            model:         ctx.model.clone(),
            messages:      messages_for_llm,
            system:        ctx.system.clone(),
            tools:         self.tools.list().iter().map(|t| neko_providers::provider::ToolDef {
                name:         t.name().to_string(),
                description:  t.description().to_string(),
                input_schema: t.input_schema(),
            }).collect(),
            max_tokens:        self.max_output_tokens,
            temperature:       None,
            top_p:             None,
            stop:              Vec::new(),
            extended_thinking: self.thinking_budget.is_some(),
            thinking_budget:   self.thinking_budget,
            reasoning_effort:  self.reasoning_effort.clone(),
        };

        let mut stream = match self.provider.stream(&req, signal.clone()).await {
            Ok(s) => s,
            Err(e) => {
                self.bus.emit(NekoEvent::AgentError {
                    session_id:   self.session_id,
                    sub_agent_id: self.sub_agent_id,
                    error:        e.to_string(),
                });
                return TurnResult::Error(e.to_string());
            }
        };

        // 累积流式内容为内容块
        let mut text_acc   = String::new();
        let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (call_id, name, input_json)
        let mut stop_reason = StopReason::EndTurn;

        use futures_util::StreamExt;
        const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        loop {
            if signal.is_cancelled() { return TurnResult::Cancelled; }
            let ev = tokio::select! {
                maybe = stream.next() => maybe,
                _ = signal.cancelled() => { return TurnResult::Cancelled; }
                _ = tokio::time::sleep(CHUNK_TIMEOUT) => {
                    return TurnResult::Error("LLM stream timeout — no chunk for 30s".into());
                }
            };
            let Some(ev) = ev else { break; };
            match ev {
                StreamEvent::Chunk(chunk) => match chunk {
                    StreamChunk::ThinkingDelta { delta } => {
                        self.bus.emit(NekoEvent::AgentReasoning {
                            session_id: self.session_id, sub_agent_id: self.sub_agent_id, delta,
                        });
                    }
                    StreamChunk::ThinkingDone { full } => {
                        self.bus.emit(NekoEvent::AgentReasoningDone {
                            session_id: self.session_id, sub_agent_id: self.sub_agent_id, full,
                        });
                    }
                    StreamChunk::TextDelta { delta } => {
                        text_acc.push_str(&delta);
                        self.bus.emit(NekoEvent::AgentText {
                            session_id: self.session_id, sub_agent_id: self.sub_agent_id, delta,
                        });
                    }
                    StreamChunk::TextDone { full } => {
                        self.bus.emit(NekoEvent::AgentTextDone {
                            session_id: self.session_id, sub_agent_id: self.sub_agent_id, full,
                        });
                    }
                    StreamChunk::ToolCallStart { call_id, tool_name } => {
                        self.bus.emit(NekoEvent::AgentToolCall {
                            session_id:   self.session_id,
                            sub_agent_id: self.sub_agent_id,
                            call_id:      call_id.clone(),
                            tool_name:    tool_name.clone(),
                            input:        serde_json::Value::Null,
                        });
                        tool_calls.push((call_id, tool_name, String::new()));
                    }
                    StreamChunk::ToolCallInput { call_id, delta } => {
                        if let Some(tc) = tool_calls.iter_mut().find(|t| t.0 == call_id) {
                            tc.2.push_str(&delta);
                        }
                    }
                    StreamChunk::ToolCallDone { call_id: _, full_input: _ } => {}
                }
                StreamEvent::Done { stop_reason: sr, usage } => {
                    stop_reason = sr;
                    ctx.input_tokens  += usage.input_tokens;
                    ctx.output_tokens += usage.output_tokens;
                    self.bus.emit(NekoEvent::ContextUpdate {
                        session_id:    self.session_id,
                        tokens:        ctx.total_tokens(),
                        message_count: ctx.messages.len(),
                    });
                }
                StreamEvent::Error(e) => {
                    self.bus.emit(NekoEvent::AgentError {
                        session_id: self.session_id, sub_agent_id: self.sub_agent_id, error: e.clone(),
                    });
                    return TurnResult::Error(e);
                }
            }
        }

        // Build assistant message from accumulated content
        let mut content_blocks: Vec<ContentBlock> = Vec::new();
        if !text_acc.is_empty() {
            content_blocks.push(ContentBlock::Text { text: text_acc });
        }
        for (call_id, tool_name, input_json) in &tool_calls {
            let input: serde_json::Value = serde_json::from_str(input_json)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            content_blocks.push(ContentBlock::ToolUse {
                tool_use_id: call_id.clone(),
                tool_name:   tool_name.clone(),
                tool_input:  input,
            });
        }

        let mut assistant_msg = Message::new(MessageRole::Assistant, content_blocks);
        assistant_msg.model = Some(ctx.model.clone());
        assistant_msg.stop_reason = Some(
            serde_json::to_value(&stop_reason)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", stop_reason)),
        );
        // 从 ContextUpdate 事件中的累积用量取差值作为本次用量（近似）
        ctx.add_message(assistant_msg.clone());
        if self.persist {
            session::append_message(self.session_id, assistant_msg).await.ok();
        }

        self.bus.emit(NekoEvent::AgentDone {
            session_id:   self.session_id,
            sub_agent_id: self.sub_agent_id,
            stop_reason:  serde_json::to_value(&stop_reason)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", stop_reason)),
        });

        // Execute tool calls if any
        if tool_calls.is_empty() {
            return TurnResult::Done { stop_reason };
        }

        // ── Phase A: permission checks + tool resolution（串行，防止并发弹窗）──────

        struct ReadyCall {
            orig_idx:  usize,
            call_id:   String,
            tool_name: String,
            input:     serde_json::Value,
            read_only: bool,
        }

        let mut result_map: std::collections::HashMap<usize, ContentBlock> =
            std::collections::HashMap::new();
        let mut ready_calls: Vec<ReadyCall> = Vec::new();

        for (idx, (call_id, tool_name, input_json)) in tool_calls.iter().enumerate() {
            if signal.is_cancelled() { return TurnResult::Cancelled; }

            let input: serde_json::Value = serde_json::from_str(input_json)
                .unwrap_or(serde_json::Value::Object(Default::default()));

            let tp = extract_tool_preview(tool_name, &input);
            let action = {
                let perms = self.permissions.lock().await;
                let path = extract_path(&input);
                let check = AccessCheck {
                    tool:        tool_name,
                    path:        path.as_deref(),
                    description: tool_name,
                    preview:     Some(tp.summary.as_str()),
                };
                let mut action = perms.evaluate(&check);
                // cwd 外路径检查仅在受限模式（ask/edit/plan）下生效；
                // build / agent 模式全自动放行，不做 cwd 边界检查。
                let restricted = matches!(
                    perms.mode(),
                    neko_core::permissions::ModeName::Ask
                        | neko_core::permissions::ModeName::Edit
                        | neko_core::permissions::ModeName::Plan
                );
                if action == PermissionAction::Allow
                    && restricted
                    && !perms.is_permissions_skipped()
                    && is_outside_cwd(tool_name, &input, &self.cwd)
                {
                    action = PermissionAction::Ask;
                }
                action
            };

            match action {
                PermissionAction::Deny => {
                    result_map.insert(idx, ContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        tool_result: serde_json::json!("permission denied by policy"),
                        is_error:    true,
                    });
                    continue;
                }
                PermissionAction::Ask => {
                    self.bus.emit(NekoEvent::ToolPermission {
                        session_id:   self.session_id,
                        sub_agent_id: self.sub_agent_id,
                        call_id:      call_id.clone(),
                        tool_name:    tool_name.clone(),
                    });
                    let decision = self.request_permission(tool_name, call_id, &tp.summary).await;

                    match decision {
                        PermissionDecision::AllowAlways => {
                            self.permissions.lock().await.allow(tool_name.clone(), None);
                        }
                        PermissionDecision::DenyAlways => {
                            self.permissions.lock().await.deny(tool_name.clone(), None);
                        }
                        _ => {}
                    }

                    if !decision.is_allow() {
                        result_map.insert(idx, ContentBlock::ToolResult {
                            tool_use_id: call_id.clone(),
                            tool_result: serde_json::json!("permission denied by user"),
                            is_error:    true,
                        });
                        continue;
                    }
                }
                PermissionAction::Allow => {}
            }

            if self.tools.get(tool_name).is_none() {
                result_map.insert(idx, ContentBlock::ToolResult {
                    tool_use_id: call_id.clone(),
                    tool_result: serde_json::json!(format!("tool not found: {}", tool_name)),
                    is_error:    true,
                });
                continue;
            }

            ready_calls.push(ReadyCall {
                orig_idx:  idx,
                call_id:   call_id.clone(),
                tool_name: tool_name.clone(),
                input,
                read_only: is_read_only(tool_name),
            });
        }

        // ── Phase B: 执行 ─────────────────────────────────────────────────────
        // 只读工具并行执行；可变工具串行执行；结果按原始顺序拼回。

        let (ro_calls, mut_calls): (Vec<ReadyCall>, Vec<ReadyCall>) =
            ready_calls.into_iter().partition(|c| c.read_only);

        // 并行执行只读组
        let ro_futs = ro_calls.into_iter().map(|rc| {
            let tool       = self.tools.get(&rc.tool_name).unwrap();
            let bus        = self.bus.clone();
            let session_id = self.session_id;
            let sub_id     = self.sub_agent_id;
            let cwd        = self.cwd.clone();
            let sig        = signal.clone();
            async move {
                let start = chrono::Utc::now().timestamp_millis();
                bus.emit(NekoEvent::ToolStart {
                    session_id, sub_agent_id: sub_id,
                    call_id: rc.call_id.clone(), tool_name: rc.tool_name.clone(),
                    input: rc.input.clone(),
                });
                let emit_bus = bus.clone();
                let emit_cid = rc.call_id.clone();
                let ctx = ToolContext {
                    cwd, session_id, signal: sig,
                    emit: Arc::new(move |key: String, value: serde_json::Value| {
                        if key == "bash_stdout" || key == "bash_stderr" {
                            let data = value.as_str().unwrap_or_default().to_string();
                            let stream = if key == "bash_stdout" { BashStream::Stdout } else { BashStream::Stderr };
                            emit_bus.emit(NekoEvent::BashOutput {
                                session_id, sub_agent_id: sub_id,
                                call_id: emit_cid.clone(), stream, data,
                            });
                        }
                    }),
                    env: std::collections::HashMap::new(),
                };
                let res = tool.execute(rc.input.clone(), &ctx).await;
                let duration_ms = (chrono::Utc::now().timestamp_millis() - start) as u64;
                let ok = res.is_ok();
                bus.emit(NekoEvent::ToolEnd {
                    session_id, sub_agent_id: sub_id,
                    call_id: rc.call_id.clone(), tool_name: rc.tool_name.clone(),
                    ok, duration_ms,
                });
                (rc.orig_idx, ContentBlock::ToolResult {
                    tool_use_id: rc.call_id,
                    tool_result: serde_json::json!(res.text()),
                    is_error:    !ok,
                })
            }
        });
        for (idx, block) in futures_util::future::join_all(ro_futs).await {
            result_map.insert(idx, block);
        }

        // 串行执行可变组
        for rc in mut_calls {
            if signal.is_cancelled() { return TurnResult::Cancelled; }
            let tool = self.tools.get(&rc.tool_name).unwrap();
            let start = chrono::Utc::now().timestamp_millis();
            self.bus.emit(NekoEvent::ToolStart {
                session_id:   self.session_id,
                sub_agent_id: self.sub_agent_id,
                call_id:      rc.call_id.clone(),
                tool_name:    rc.tool_name.clone(),
                input:        rc.input.clone(),
            });
            let emit_bus     = self.bus.clone();
            let emit_session = self.session_id;
            let emit_sub     = self.sub_agent_id;
            let emit_call_id = rc.call_id.clone();
            let tool_ctx = ToolContext {
                cwd:        self.cwd.clone(),
                session_id: self.session_id,
                signal:     signal.clone(),
                emit:       Arc::new(move |key: String, value: serde_json::Value| {
                    if key == "bash_stdout" || key == "bash_stderr" {
                        let data = value.as_str().unwrap_or_default().to_string();
                        let stream = if key == "bash_stdout" { BashStream::Stdout } else { BashStream::Stderr };
                        emit_bus.emit(NekoEvent::BashOutput {
                            session_id:   emit_session,
                            sub_agent_id: emit_sub,
                            call_id:      emit_call_id.clone(),
                            stream,
                            data,
                        });
                    }
                }),
                env: std::collections::HashMap::new(),
            };
            let result = tool.execute(rc.input.clone(), &tool_ctx).await;
            let duration_ms = (chrono::Utc::now().timestamp_millis() - start) as u64;
            let ok = result.is_ok();
            self.bus.emit(NekoEvent::ToolEnd {
                session_id:   self.session_id,
                sub_agent_id: self.sub_agent_id,
                call_id:      rc.call_id.clone(),
                tool_name:    rc.tool_name.clone(),
                ok,
                duration_ms,
            });
            result_map.insert(rc.orig_idx, ContentBlock::ToolResult {
                tool_use_id: rc.call_id,
                tool_result: serde_json::json!(result.text()),
                is_error:    !ok,
            });
        }

        // 按原始顺序重建结果列表
        let tool_results: Vec<ContentBlock> = (0..tool_calls.len())
            .filter_map(|i| result_map.remove(&i))
            .collect();

        let tool_result_msg = Message::new(MessageRole::ToolResult, tool_results);
        ctx.add_message(tool_result_msg.clone());
        if self.persist {
            session::append_message(self.session_id, tool_result_msg).await.ok();
        }

        TurnResult::Continue
    }

    /// 向前端发起权限请求并等待用户决定。
    /// 无通道时返回 DenyOnce（安全默认）。
    async fn request_permission(
        &self,
        tool_name: &str,
        call_id:   &str,
        preview:   &str,
    ) -> PermissionDecision {
        let Some(tx) = &self.permission_tx else {
            debug!(tool = %tool_name, "no permission channel; denying");
            return PermissionDecision::DenyOnce;
        };

        let (responder, rx) = oneshot::channel();
        let req = PermissionRequest {
            tool_name:     tool_name.to_string(),
            call_id:       call_id.to_string(),
            input_preview: preview.to_string(),
            responder,
        };

        if tx.send(req).await.is_err() {
            debug!("permission channel closed; denying");
            return PermissionDecision::DenyOnce;
        }

        rx.await.unwrap_or(PermissionDecision::DenyOnce)
    }
}

/// 从工具输入中提取 path 字段（用于路径级权限匹配）。
fn extract_path(input: &serde_json::Value) -> Option<String> {
    input.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 文件读取类工具：路径在 cwd 之外时需要用户审批。
const FILE_READ_TOOLS: &[&str] = &["read_file", "glob", "grep", "tree"];

/// 检查工具操作的路径是否超出 cwd 范围。
/// 只对 FILE_READ_TOOLS 生效；glob/grep 使用 pattern/path 字段。
/// canonicalize 失败时放宽（返回 false）——避免符号链接等场景下误判。
fn is_outside_cwd(tool_name: &str, input: &serde_json::Value, cwd: &std::path::Path) -> bool {
    if !FILE_READ_TOOLS.contains(&tool_name) {
        return false;
    }
    let raw = input.get("path")
        .or_else(|| input.get("pattern"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if raw.is_empty() {
        return false;
    }
    let pb = std::path::Path::new(raw);
    let resolved = if pb.is_absolute() { pb.to_path_buf() } else { cwd.join(pb) };
    // 两者都必须 canonicalize 成功才能可靠比较；任一失败则放宽（视为 cwd 内）
    match (resolved.canonicalize(), cwd.canonicalize()) {
        (Ok(canon), Ok(cwd_canon)) => !canon.starts_with(&cwd_canon),
        _ => false,
    }
}

/// Prune：截断老的 tool_result 内容，减少发给 LLM 的 token 量。
///
/// 保留最近 `KEEP_RECENT` 条消息的 tool_result 完整，
/// 更早的 tool_result 截断到 `TRUNCATE_TO` 字符。
/// 只修改传入的 messages（通常是 clone），不影响原始消息序列。
fn prune_tool_results(messages: &mut [Message]) {
    const KEEP_RECENT: usize = 12;
    const TRUNCATE_TO: usize = 500;

    let cutoff = messages.len().saturating_sub(KEEP_RECENT);
    for msg in messages[..cutoff].iter_mut() {
        for block in msg.content.iter_mut() {
            if let ContentBlock::ToolResult { tool_result, .. } = block {
                let s = tool_result.to_string();
                if s.len() > TRUNCATE_TO * 2 {
                    let truncated = format!(
                        "{}\n...[truncated — {} chars removed]",
                        &s[..TRUNCATE_TO.min(s.len())],
                        s.len() - TRUNCATE_TO,
                    );
                    *tool_result = serde_json::Value::String(truncated);
                }
            }
        }
    }
}

