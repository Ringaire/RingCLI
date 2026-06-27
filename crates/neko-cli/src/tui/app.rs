use std::sync::Arc;

use anyhow::Result;
use crossterm::{
    event::{
        DisableBracketedPaste, EnableBracketedPaste,
        Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, Mutex as TokioMutex};
use tokio_util::sync::CancellationToken;

use neko_core::events::NekoEvent;
use neko_core::session;
use neko_core::tools::Message;
use neko_providers::provider::{DEFAULT_CONTEXT_WINDOW, DEFAULT_THINKING_BUDGET};

use neko_engine::{
    AgentContext, PermissionDecision, PermissionRequest, TurnResult,
};
use neko_engine::agent::tool_preview::extract_tool_preview;
use crate::args::Args;
use crate::bootstrap::BootstrappedRuntime;
use crate::repl::history::History;

use super::layout::AppLayout;
use super::widgets::{
    chat::ChatWidget,
    input::InputWidget,
    mode_picker::ModePickerModal,
    model_picker::ModelPickerModal,
    permission::PermissionModal,
    provider_setup::{ProviderRow, ProviderSetupModal, SetupAction},
    session_picker::{SessionEntry, SessionPickerModal},
    suggestions,
    tasks,
    welcome::render_welcome,
};
use super::theme::{SPINNER, MUTED, ERR, WARN, THINK};

/// 待处理的权限请求：UI 数据 + 回应通道。
struct PendingPermission {
    modal:     PermissionModal,
    responder: tokio::sync::oneshot::Sender<PermissionDecision>,
}

/// 子 agent 树中已注册的 sub_agent_id 列表
fn sub_agent_ids(chat: &ChatWidget) -> Vec<uuid::Uuid> {
    let mut ids: Vec<uuid::Uuid> = chat.bubbles.iter()
        .filter_map(|b| b.sub_agent)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

struct AppState {
    model:            String,
    mode:             String,
    cwd:              String,
    session_id:       uuid::Uuid,
    skip_perms:       bool,
    tokens:           u64,
    context_window:   u64,
    is_running:       bool,
    is_thinking:      bool,
    spinner_idx:      usize,
    chat:             ChatWidget,
    input:            InputWidget,
    pending:          Option<PendingPermission>,
    model_picker:     Option<ModelPickerModal>,
    mode_picker:      Option<ModePickerModal>,
    provider_setup:   Option<ProviderSetupModal>,
    signal:           Option<CancellationToken>,
    status_msg:       Option<String>,
    turn_start_ms:    Option<i64>,
    tasks:            Vec<super::widgets::tasks::TodoView>,
    show_tasks:       bool,
    tasks_auto_shown: bool,
    /// 会话选择器浮层。
    session_picker:   Option<SessionPickerModal>,
    /// 当前 `/` 命令补全建议（空 = 不显示）。
    suggestions:      Vec<crate::repl::commands::Suggestion>,
    /// 选中的建议索引。
    suggestion_idx:   usize,
    show_token_count: bool,
    /// 是否启用 extended thinking（发送给模型）
    think_enabled:    bool,
    /// 是否在聊天中展示 reasoning 内容
    think_show:       bool,
    /// thinking token 预算
    think_budget:     u32,
    /// reasoning effort 级别（low/medium/high/max），None = 不发送
    effort:           Option<String>,
    /// 自主循环状态（/loop 命令）
    loop_state:       Option<neko_core::session::loop_state::LoopState>,
    /// 运行时用户输入的消息队列（Agent 完成后续按顺序消费）
    queued_messages:  Vec<String>,
    /// Ctrl+C 首次按下后进入 pending 态，再次按下才退出
    exit_pending:     bool,
    /// 当前选中的子 agent 视图（None = 主 agent）
    active_sub_agent: Option<uuid::Uuid>,
    /// 会话重命名输入状态
    rename_input:     Option<super::widgets::session_picker::RenameState>,
    /// 会话操作待确认：删除/fork 需二次确认
    session_action:   Option<super::widgets::session_picker::SessionAction>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        model:            impl Into<String>,
        mode:             impl Into<String>,
        cwd:              impl Into<String>,
        session_id:       uuid::Uuid,
        skip_perms:       bool,
        show_token_count: bool,
    ) -> Self {
        Self {
            model:            model.into(),
            mode:             mode.into(),
            cwd:              cwd.into(),
            session_id,
            skip_perms,
            tokens:           0,
            context_window:   DEFAULT_CONTEXT_WINDOW,
            is_running:       false,
            is_thinking:      false,
            spinner_idx:      0,
            chat:             ChatWidget::new(),
            input:            InputWidget::new(),
            pending:          None,
            model_picker:     None,
            mode_picker:      None,
            provider_setup:   None,
            signal:           None,
            status_msg:       None,
            turn_start_ms:    None,
            tasks:            Vec::new(),
            show_tasks:       false,
            tasks_auto_shown: false,
            session_picker:   None,
            suggestions:      Vec::new(),
            suggestion_idx:   0,
            show_token_count,
            think_enabled:    false,
            think_show:       true,
            think_budget:     DEFAULT_THINKING_BUDGET,
            effort:           None,
            loop_state:       None,
            queued_messages:  Vec::new(),
            exit_pending:     false,
            active_sub_agent: None,
            rename_input:     None,
            session_action:   None,
        }
    }

    /// 刷新任务列表（同步读盘）；首次出现任务时自动弹出面板。
    fn refresh_tasks(&mut self) {
        self.tasks = super::widgets::tasks::load_todos(self.session_id);
        if !self.tasks.is_empty() && !self.tasks_auto_shown {
            self.show_tasks = true;
            self.tasks_auto_shown = true;
        }
    }

    /// 把历史会话消息渲染到 chat。
    fn load_history_messages(&mut self, messages: &[Message]) {
        use neko_core::tools::{ContentBlock, MessageRole};
        for m in messages {
            match m.role {
                MessageRole::User => {
                    let text = collect_text(&m.content);
                    if !text.is_empty() {
                        self.chat.add_user(text);
                    }
                }
                MessageRole::Assistant => {
                    let text = collect_text(&m.content);
                    if !text.is_empty() {
                        self.chat.append_assistant(None, &text);
                        self.chat.end_turn();
                    }
                    for blk in &m.content {
                        if let ContentBlock::ToolUse { tool_use_id, tool_name, tool_input } = blk {
                            let preview = preview_value(tool_name, tool_input);
                            self.chat.add_tool(None, tool_use_id, tool_name, &preview);
                            self.chat.complete_tool(tool_use_id, true, 0);
                        }
                    }
                }
                MessageRole::ToolResult => {}
            }
        }
        self.chat.end_turn();
    }

    /// 把总线事件映射到界面状态。主/次 agent 由 sub_agent_id 区分。
    fn apply_event(&mut self, ev: &NekoEvent) {
        let sub = ev.sub_agent_id();
        match ev {
            NekoEvent::AgentThinking { .. } => {
                self.is_thinking = true;
            }
            NekoEvent::AgentReasoning { delta, .. } => {
                self.chat.append_reasoning(sub, delta);
            }
            NekoEvent::AgentText { delta, .. } => {
                self.is_thinking = false;
                self.chat.append_assistant(sub, delta);
            }
            NekoEvent::AgentSpawned { sub_agent_id, role, model, task, .. } => {
                let role_s = role.as_deref().unwrap_or("balanced");
                self.chat.add_spawn(*sub_agent_id, role_s, model, task);
            }
            NekoEvent::AgentToolCall { call_id, tool_name, input, .. } => {
                let preview = preview_value(tool_name, input);
                self.chat.add_tool(sub, call_id, tool_name, &preview);
            }
            NekoEvent::ToolStart { call_id, tool_name, input, .. } => {
                let preview = preview_value(tool_name, input);
                self.chat.add_tool(sub, call_id, tool_name, &preview);
            }
            NekoEvent::ToolEnd { call_id, ok, duration_ms, .. } => {
                self.chat.complete_tool(call_id, *ok, *duration_ms);
            }
            NekoEvent::BashOutput { call_id, stream, data, .. } => {
                let is_stderr = matches!(stream, neko_core::events::BashStream::Stderr);
                self.chat.append_bash_output(call_id, is_stderr, data);
            }
            NekoEvent::AgentError { error, .. } => {
                self.is_thinking = false;
                self.chat.add_error(sub, error.clone());
            }
            NekoEvent::AgentDone { .. } => {
                // 仅主 agent done 才解除 thinking / 固化轮次
                if sub.is_none() {
                    self.is_thinking = false;
                }
            }
            NekoEvent::ContextUpdate { tokens, .. } => {
                self.tokens = *tokens;
            }
            _ => {}
        }
    }
}

fn collect_text(content: &[neko_core::tools::ContentBlock]) -> String {
    use neko_core::tools::ContentBlock;
    content.iter()
        .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
        .collect::<Vec<_>>()
        .join("\n")
}

fn preview_value(tool_name: &str, input: &serde_json::Value) -> String {
    extract_tool_preview(tool_name, input).summary
}

/// TUI 主入口。
pub async fn run_with_runtime(mut runtime: BootstrappedRuntime, args: &Args) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend  = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    let result = run_loop(&mut runtime, args, &mut term).await;

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableBracketedPaste)?;
    term.show_cursor()?;

    result
}

async fn run_loop<B: ratatui::backend::Backend>(
    runtime: &mut BootstrappedRuntime,
    args:    &Args,
    term:    &mut Terminal<B>,
) -> Result<()> {
    let ctx = Arc::new(TokioMutex::new(AgentContext::from_session(
        &runtime.session,
        runtime.model.clone(),
        Some(runtime.system_prompt.clone()),
    )));

    let cwd_str = runtime.session.meta.cwd.to_string_lossy().into_owned();
    let mut state = AppState::new(
        runtime.model.clone(),
        runtime.mode.to_string(),
        cwd_str,
        runtime.session.meta.id,
        runtime.skip_perms,
        runtime.config.ui.show_token_count,
    );
    {
        let guard = ctx.lock().await;
        state.load_history_messages(&guard.messages);
    }

    if runtime.skip_perms {
        state.chat.add_system("--dangerously-skip-permissions active: all tool calls auto-approved");
    }

    // 冷启动：未配置任何 provider → 进入 setup-required 态，提示并自动打开 /connect 向导。
    if runtime.provider.is_none() {
        state.chat.add_system("Setup Required — neko needs a model provider before it can start.");
        state.chat.add_system("  Pick a provider below (Enter), or press Esc and run /connect later.");
        state.provider_setup = Some(build_setup_modal(&state));
    }

    let mut history = History::load().await;

    let mut term_events = EventStream::new();
    let mut bus_sub     = runtime.bus.subscribe();
    let (perm_tx, mut perm_rx) = mpsc::channel::<PermissionRequest>(8);
    let (done_tx, mut done_rx) = mpsc::channel::<TurnResult>(8);
    // 模型列表在后台拉取（list_models 可能走网络），拉完通过此通道回传 picker，
    // 避免在事件循环里 await 网络调用导致 UI 冻结。
    let (picker_tx, mut picker_rx) = mpsc::channel::<ModelPickerModal>(1);
    // /connect 向导拉取 /models 列表（走网络）的后台结果回传通道。
    let (setup_tx, mut setup_rx) = mpsc::channel::<Result<(Vec<String>, Option<String>), String>>(1);

    // 初始 prompt
    if let Some(prompt) = args.prompt.clone() {
        if !prompt.trim().is_empty() {
            history.push(&prompt).await;
            start_turn(runtime, &ctx, &mut state, &perm_tx, &done_tx, prompt).await;
        }
    }

    let mut spinner_tick = tokio::time::interval(std::time::Duration::from_millis(80));

    loop {
        draw(term, &mut state)?;

        tokio::select! {
            maybe_ev = term_events.next() => {
                match maybe_ev {
                    Some(Ok(ev)) => {
                        match handle_term_event(ev, runtime, &ctx, &mut state, &mut history, &perm_tx, &done_tx, &picker_tx, &setup_tx).await {
                            Control::Quit => break,
                            Control::Continue => {}
                        }
                    }
                    Some(Err(_)) | None => break,
                }
            }
            bev = bus_sub.recv() => {
                if let Ok(bev) = bev {
                    state.apply_event(&bev);
                }
            }
            Some(req) = perm_rx.recv() => {
                if state.skip_perms {
                    let _ = req.responder.send(PermissionDecision::AllowOnce);
                } else {
                    state.pending = Some(PendingPermission {
                        modal:     PermissionModal::new(req.tool_name.clone(), req.input_preview.clone()),
                        responder: req.responder,
                    });
                }
            }
            Some(result) = done_rx.recv() => {
                finish_turn(&mut state, result);
                // Auto-compact：上下文占用 ≥ 80% 时自动压缩
                if state.tokens > 0 && state.context_window > 0
                    && state.tokens * 100 / state.context_window >= 80
                {
                    compact_tui(runtime, &ctx, &mut state).await;
                }
                // 消费队列：一轮完成后自动处理下一条排队消息
                if !state.queued_messages.is_empty() {
                    let text = state.queued_messages.remove(0);
                    start_turn(runtime, &ctx, &mut state, &perm_tx, &done_tx, text).await;
                } else if let Some(ls) = &mut state.loop_state {
                    // 自主循环：检查完成 → 自动继续
                    ls.advance();
                    let done = state.chat.bubbles.last()
                        .filter(|b| b.kind == super::widgets::chat::BubbleKind::Assistant)
                        .map(|b| b.content.contains(neko_core::LOOP_DONE_MARKER))
                        .unwrap_or(false);
                    if done {
                        state.chat.add_system(format!("⟳ loop complete ({} turns)", ls.current_turn));
                        state.loop_state = None;
                    } else if ls.is_exhausted() {
                        state.chat.add_system(format!("⟳ loop exhausted (max {} turns)", ls.max_turns));
                        state.loop_state = None;
                    } else {
                        let prompt = ls.build_continuation_prompt();
                        state.chat.add_system(format!("⟳ loop {}/{}", ls.current_turn + 1, ls.max_turns));
                        start_turn(runtime, &ctx, &mut state, &perm_tx, &done_tx, prompt).await;
                    }
                }
            }
            Some(picker) = picker_rx.recv() => {
                // 后台拉取完成：清除 loading 并打开选择器
                if state.status_msg.as_deref() == Some("loading models…") {
                    state.status_msg = None;
                }
                state.model_picker = Some(picker);
            }
            Some(result) = setup_rx.recv() => {
                // /connect 向导：模型拉取 / OAuth 结果回传
                match result {
                    Ok((models, oauth_api_key)) => {
                        // OAuth 成功：先设置 API key 到 provider_setup
                        if let Some(key) = oauth_api_key {
                            if let Some(m) = &mut state.provider_setup {
                                m.apply_oauth_result(Ok(key));
                            }
                        }
                        let action = state.provider_setup.as_mut().map(|m| m.apply_models(models));
                        if let Some(SetupAction::Commit) = action {
                            finish_setup(runtime, &ctx, &mut state).await;
                        }
                    }
                    Err(e) => {
                        if let Some(m) = &mut state.provider_setup { m.fetch_failed(e); }
                    }
                }
            }
            _ = spinner_tick.tick() => {
                if state.is_running {
                    state.spinner_idx = (state.spinner_idx + 1) % SPINNER.len();
                }
            }
        }
    }

    Ok(())
}

enum Control {
    Continue,
    Quit,
}

#[allow(clippy::too_many_arguments)]
async fn handle_term_event(
    ev:       Event,
    runtime:  &mut BootstrappedRuntime,
    ctx:      &Arc<TokioMutex<AgentContext>>,
    state:    &mut AppState,
    history:  &mut History,
    perm_tx:  &mpsc::Sender<PermissionRequest>,
    done_tx:  &mpsc::Sender<TurnResult>,
    picker_tx: &mpsc::Sender<ModelPickerModal>,
    setup_tx:  &mpsc::Sender<Result<(Vec<String>, Option<String>), String>>,
) -> Control {
    match ev {
        Event::Paste(text) => {
            if state.pending.is_none() {
                if let Some(m) = &mut state.provider_setup {
                    // provider 向导激活：粘贴到向导输入框（API key 等）
                    m.insert_str(&text);
                } else if let Some(picker) = &mut state.model_picker {
                    // model picker 激活：粘贴到搜索过滤器
                    picker.append_filter(&text);
                } else if state.mode_picker.is_none() && state.session_picker.is_none() {
                    // 正常输入框
                    let processed = crate::repl::file_complete::process_paste(&text);
                    state.input.insert_str(&processed);
                }
            }
            Control::Continue
        }
        Event::Key(ke) if ke.kind == KeyEventKind::Press => {
            // 权限浮层优先处理按键
            if state.pending.is_some() {
                handle_permission_key(ke, runtime, state).await;
                return Control::Continue;
            }
            // /connect 向导优先处理按键
            if state.provider_setup.is_some() {
                return handle_provider_setup_key(ke, runtime, ctx, state, setup_tx).await;
            }
            // 会话选择器优先处理按键
            if state.session_picker.is_some() && state.rename_input.is_none() {
                return handle_session_picker_key(ke, runtime, ctx, state).await;
            }
            // 重命名输入状态
            if state.rename_input.is_some() {
                return handle_rename_key(ke, state).await;
            }
            // 模型选择器优先处理按键
            if state.model_picker.is_some() {
                return handle_model_picker_key(ke, runtime, ctx, state).await;
            }
            // 模式选择器优先处理按键
            if state.mode_picker.is_some() {
                return handle_mode_picker_key(ke, runtime, state).await;
            }
            handle_key(ke, runtime, ctx, state, history, perm_tx, done_tx, picker_tx).await
        }
        Event::Mouse(me) => {
            match me.kind {
                MouseEventKind::ScrollUp   => { state.chat.scroll_up(3); }
                MouseEventKind::ScrollDown => { state.chat.scroll_down(3); }
                _ => {}
            }
            Control::Continue
        }
        _ => Control::Continue,
    }
}

async fn handle_permission_key(ke: KeyEvent, runtime: &BootstrappedRuntime, state: &mut AppState) {
    // 方向键导航
    match ke.code {
        KeyCode::Up => {
            if let Some(p) = &mut state.pending { p.modal.move_up(); }
            return;
        }
        KeyCode::Down => {
            if let Some(p) = &mut state.pending { p.modal.move_down(); }
            return;
        }
        _ => {}
    }

    let choice: Option<usize> = match ke.code {
        KeyCode::Char('1') => Some(0),
        KeyCode::Char('2') => Some(1),
        KeyCode::Char('3') => Some(2),
        KeyCode::Char('4') => Some(3),
        KeyCode::Enter     => state.pending.as_ref().map(|p| p.modal.cursor()),
        KeyCode::Esc       => Some(3),
        _ => None,
    };

    if let Some(idx) = choice {
        if let Some(p) = state.pending.take() {
            let tool = p.modal.tool_name.clone();
            let command = p.modal.preview.clone();
            let decision = match idx {
                0 => PermissionDecision::AllowOnce,
                1 => PermissionDecision::AllowAlways,
                _ => PermissionDecision::DenyOnce,
            };
            let _ = p.responder.send(decision);
            if idx == 1 {
                let cmd = if command.is_empty() { None } else { Some(command.clone()) };
                runtime.permissions.lock().await.allow(&tool, cmd);
                state.chat.add_system("permission granted (always for this command)");
            }
        }
    }
}

async fn handle_model_picker_key(
    ke:      KeyEvent,
    runtime: &mut BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
) -> Control {
    let ctrl = ke.modifiers.contains(KeyModifiers::CONTROL);
    match ke.code {
        // 搜索：字符输入追加到 filter
        KeyCode::Char(c) if !ctrl => {
            if let Some(picker) = &mut state.model_picker {
                picker.push_filter_char(c);
            }
        }
        // 搜索：Backspace 删除 filter 最后一个字符
        KeyCode::Backspace => {
            if let Some(picker) = &mut state.model_picker {
                if !picker.filter().is_empty() {
                    picker.pop_filter_char();
                }
            }
        }
        KeyCode::Up => {
            if let Some(picker) = &mut state.model_picker {
                picker.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(picker) = &mut state.model_picker {
                picker.move_down();
            }
        }
        KeyCode::PageUp => {
            if let Some(picker) = &mut state.model_picker {
                picker.page_up();
            }
        }
        KeyCode::PageDown => {
            if let Some(picker) = &mut state.model_picker {
                picker.page_down();
            }
        }
        KeyCode::Home => {
            if let Some(picker) = &mut state.model_picker {
                picker.move_home();
            }
        }
        KeyCode::End => {
            if let Some(picker) = &mut state.model_picker {
                picker.move_end();
            }
        }
        KeyCode::Enter => {
            if let Some(picker) = state.model_picker.take() {
                if let Some(model_ref) = picker.selected_ref() {
                    switch_model(runtime, ctx, state, model_ref).await;
                }
            }
        }
        KeyCode::Esc => {
            state.model_picker = None;
        }
        _ => {}
    }
    Control::Continue
}

/// 模式选择器按键处理。
async fn handle_mode_picker_key(
    ke:      KeyEvent,
    runtime: &mut BootstrappedRuntime,
    state:   &mut AppState,
) -> Control {
    match ke.code {
        KeyCode::Up => {
            if let Some(picker) = &mut state.mode_picker {
                picker.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(picker) = &mut state.mode_picker {
                picker.move_down();
            }
        }
        KeyCode::Enter => {
            if let Some(picker) = state.mode_picker.take() {
                let mode = picker.selected();
                runtime.mode = mode;
                runtime.permissions.lock().await.set_mode(mode);
                state.mode = mode.to_string();
            }
        }
        KeyCode::Esc => {
            state.mode_picker = None;
        }
        _ => {}
    }
    Control::Continue
}

/// 会话选择器按键处理。
async fn handle_session_picker_key(
    ke:      KeyEvent,
    runtime: &mut BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
) -> Control {
    let ctrl = ke.modifiers.contains(KeyModifiers::CONTROL);
    match ke.code {
        KeyCode::Up => {
            if let Some(picker) = &mut state.session_picker {
                picker.move_up();
            }
            // 移动时取消待确认操作
            state.session_action = None;
        }
        KeyCode::Down => {
            if let Some(picker) = &mut state.session_picker {
                picker.move_down();
            }
            // 移动时取消待确认操作
            state.session_action = None;
        }
        KeyCode::Enter => {
            if let Some(picker) = state.session_picker.take() {
                state.session_action = None;
                if let Some(id) = picker.selected_id() {
                    resume_session_tui(runtime, ctx, state, id).await;
                }
            }
        }
        // Ctrl+D: 删除（二次确认）
        KeyCode::Char('d') if ctrl => {
            if let Some(super::widgets::session_picker::SessionAction::Delete { id, .. }) = state.session_action.take() {
                // 已确认 → 执行删除
                delete_session_tui(state, id).await;
            } else if let Some(picker) = &state.session_picker {
                if let Some(id) = picker.selected_id() {
                    let title = picker.selected_title().unwrap_or_default().to_string();
                    state.session_action = Some(super::widgets::session_picker::SessionAction::Delete { id, title });
                }
            }
        }
        // Ctrl+R: 重命名
        KeyCode::Char('r') if ctrl => {
            state.session_action = None;
            if let Some(picker) = &state.session_picker {
                if let Some(id) = picker.selected_id() {
                    let name = picker.selected_title().unwrap_or_default().to_string();
                    rename_session_tui(state, id, &name).await;
                }
            }
        }
        // Ctrl+F: Fork（二次确认）
        KeyCode::Char('f') if ctrl => {
            if let Some(super::widgets::session_picker::SessionAction::Fork { id }) = state.session_action.take() {
                // 已确认 → 执行 fork
                state.session_picker = None;
                fork_session_tui(runtime, ctx, state, id).await;
            } else if let Some(picker) = &state.session_picker {
                if let Some(id) = picker.selected_id() {
                    state.session_action = Some(super::widgets::session_picker::SessionAction::Fork { id });
                }
            }
        }
        KeyCode::Esc => {
            if state.session_action.is_some() {
                // 有待确认操作时 Esc 只取消操作，不关闭 picker
                state.session_action = None;
            } else {
                state.session_picker = None;
            }
        }
        _ => {}
    }
    Control::Continue
}

/// 重命名输入处理：Enter 确认 / Esc 取消 / 字符输入
async fn handle_rename_key(ke: KeyEvent, state: &mut AppState) -> Control {
    let ctrl = ke.modifiers.contains(KeyModifiers::CONTROL);
    match ke.code {
        KeyCode::Enter => {
            if let Some(rs) = state.rename_input.take() {
                let new_title = rs.current_title.trim().to_string();
                if !new_title.is_empty() {
                    let id = rs.session_id;
                    let _ = neko_core::session::rename_session(id, new_title.clone()).await;
                    state.chat.add_system(format!("renamed session to: {new_title}"));
                    // 刷新 picker 列表
                    refresh_session_picker(state).await;
                }
            }
        }
        KeyCode::Esc => {
            state.rename_input = None;
        }
        KeyCode::Backspace => {
            if let Some(rs) = &mut state.rename_input {
                rs.current_title.pop();
            }
        }
        KeyCode::Char(c) if !ctrl => {
            if let Some(rs) = &mut state.rename_input {
                rs.current_title.push(c);
            }
        }
        _ => {}
    }
    Control::Continue
}

// ── /connect 向导 ─────────────────────────────────────────────────────────────

async fn handle_provider_setup_key(
    ke:       KeyEvent,
    runtime:  &mut BootstrappedRuntime,
    ctx:      &Arc<TokioMutex<AgentContext>>,
    state:    &mut AppState,
    setup_tx: &mpsc::Sender<Result<(Vec<String>, Option<String>), String>>,
) -> Control {
    let ctrl = ke.modifiers.contains(KeyModifiers::CONTROL);
    let action = match ke.code {
        KeyCode::Up   => { if let Some(m) = &mut state.provider_setup { m.move_up(); }   SetupAction::Stay }
        KeyCode::Down => { if let Some(m) = &mut state.provider_setup { m.move_down(); } SetupAction::Stay }
        KeyCode::Enter => state.provider_setup.as_mut().map(|m| m.confirm()).unwrap_or(SetupAction::Stay),
        KeyCode::Esc   => state.provider_setup.as_mut().map(|m| m.cancel()).unwrap_or(SetupAction::Cancel),
        // Backspace；Ctrl+H 是部分终端对 Backspace 的编码。
        KeyCode::Backspace => { if let Some(m) = &mut state.provider_setup { m.backspace(); } SetupAction::Stay }
        KeyCode::Char('h') if ctrl => { if let Some(m) = &mut state.provider_setup { m.backspace(); } SetupAction::Stay }
        KeyCode::Char('u') if ctrl => { if let Some(m) = &mut state.provider_setup { m.clear_line(); }  SetupAction::Stay }
        KeyCode::Char('w') if ctrl => { if let Some(m) = &mut state.provider_setup { m.delete_word(); } SetupAction::Stay }
        KeyCode::Char(c) if !ctrl => { if let Some(m) = &mut state.provider_setup { m.input_char(c); } SetupAction::Stay }
        _ => SetupAction::Stay,
    };

    match action {
        SetupAction::Stay   => {}
        SetupAction::Cancel => { state.provider_setup = None; }
        SetupAction::Fetch  => spawn_fetch_models(state, runtime, setup_tx),
        SetupAction::OAuthBrowser => spawn_oauth(state, runtime, setup_tx, true),
        SetupAction::OAuthDevice  => spawn_oauth(state, runtime, setup_tx, false),
        SetupAction::Commit => finish_setup(runtime, ctx, state).await,
    }
    Control::Continue
}

/// 后台构造临时 provider 拉取 `/models`，结果经 `setup_tx` 回传（不阻塞事件循环）。
fn spawn_fetch_models(
    state:    &AppState,
    runtime:  &BootstrappedRuntime,
    setup_tx: &mpsc::Sender<Result<(Vec<String>, Option<String>), String>>,
) {
    let Some(m) = &state.provider_setup else { return; };
    let kind          = m.kind();
    let id            = m.provider_id().to_string();
    let api_key       = m.api_key().to_string();
    let base_url      = m.base_url();
    let default_model = m.default_model();
    let proxy         = runtime.config.proxy.clone();
    let tx            = setup_tx.clone();

    tokio::spawn(async move {
        let result = match neko_providers::build_probe_provider(
            proxy.as_deref(), &kind, &id, api_key, base_url, default_model,
        ) {
            Some(p) => match p.list_models().await {
                Ok(models) => {
                    let ids: Vec<String> = models.into_iter().map(|mi| mi.id).collect();
                    Ok((ids, None))
                }
                Err(e)     => Err(format!("{e}")),
            },
            None => Err("could not build provider for model fetch".to_string()),
        };
        let _ = tx.send(result).await;
    });
}

/// 后台执行 ChatGPT OAuth2 登录，成功后用换来的 API key 拉取模型列表。
fn spawn_oauth(
    state:    &AppState,
    runtime:  &BootstrappedRuntime,
    setup_tx: &mpsc::Sender<Result<(Vec<String>, Option<String>), String>>,
    browser:  bool,
) {
    let proxy = runtime.config.proxy.clone();
    let tx    = setup_tx.clone();

    tokio::spawn(async move {
        let client = neko_providers::provider::build_http_client(
            proxy.as_deref(),
            neko_providers::provider::DEFAULT_CONNECT_TIMEOUT_SECS,
        );

        // 1. OAuth2 登录
        let auth_result = if browser {
            neko_providers::providers::openai::oauth::login_browser(&client).await
        } else {
            neko_providers::providers::openai::oauth::login_device(&client).await
        };

        let auth_data = match auth_result {
            Ok(d) => d,
            Err(e) => {
                let _ = tx.send(Err(e.to_string())).await;
                return;
            }
        };

        // 2. 持久化 auth data
        let _ = neko_providers::providers::openai::oauth::save_auth(&auth_data).await;

        // 3. 获取 API key
        let api_key = match auth_data.api_key() {
            Some(k) => k.to_string(),
            None => {
                let _ = tx.send(Err("OAuth succeeded but no API key obtained".into())).await;
                return;
            }
        };

        // 4. 用 API key 构造 probe provider 拉取模型
        let probe = neko_providers::build_probe_provider(
            proxy.as_deref(),
            &neko_providers::catalog::ProviderKind::OpenAi,
            "openai",
            api_key.clone(),
            Some("https://api.openai.com/v1".to_string()),
            Some("gpt-4o".to_string()),
        );

        let models: Vec<String> = match probe {
            Some(p) => p.list_models().await
                .map(|ms| ms.into_iter().map(|m| m.id).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };

        // 5. 回传模型 + API key
        let _ = tx.send(Ok((models, Some(api_key)))).await;
    });
}

/// 向导落盘 + 热重载。成功则关闭向导并推送 system 消息；失败则在向导内显示错误。
async fn finish_setup(
    runtime: &mut BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
) {
    let Some((provider_id, api_key, base_url, is_custom, model, fetched)) =
        state.provider_setup.as_ref().map(|m| (
            m.provider_id().to_string(),
            m.api_key().to_string(),
            m.base_url(),
            m.is_custom(),
            m.chosen_model().unwrap_or_default(),
            m.fetched_models().to_vec(),
        ))
    else { return; };

    if model.is_empty() {
        if let Some(m) = &mut state.provider_setup { m.apply_saved(Err("no model selected".into())); }
        return;
    }

    let mut cfg = neko_core::load_user_config().await;
    let providers = cfg.providers.get_or_insert_with(Default::default);
    let mut entry = neko_core::ProviderEntry::default();
    if !api_key.is_empty() { entry.api_key = Some(api_key); }
    if is_custom { entry.base_url = base_url; }   // preset 的 base_url 来自 catalog，不入库
    providers.insert(provider_id.clone(), entry);
    cfg.model = Some(format!("{provider_id}/{model}"));
    if !fetched.is_empty() {
        cfg.models.get_or_insert_with(Default::default).insert(provider_id.clone(), fetched);
    }

    let cwd = std::path::PathBuf::from(&state.cwd);
    match crate::connect::apply_config_reload(runtime, &cwd, &cfg, &provider_id, &model).await {
        Ok(()) => {
            {
                let mut g = ctx.lock().await;
                g.model = model.clone();
                g.system = Some(runtime.system_prompt.clone());
            }
            state.model = model.clone();
            state.chat.add_system(format!("Connected — switched to {provider_id}/{model}"));
            state.provider_setup = None;
        }
        Err(e) => { if let Some(m) = &mut state.provider_setup { m.apply_saved(Err(e)); } }
    }
}

/// `/connect <provider> <key> [url]` 快速形态：直接写配置 + 热重载，无 UI 步骤。
async fn quick_connect(
    runtime:  &mut BootstrappedRuntime,
    ctx:      &Arc<TokioMutex<AgentContext>>,
    state:    &mut AppState,
    provider: String,
    api_key:  Option<String>,
    base_url: Option<String>,
) {
    use crate::connect::ConnectResult;
    let cwd = std::path::PathBuf::from(&state.cwd);
    match crate::connect::quick_connect(runtime, &cwd, &provider, api_key, base_url).await {
        ConnectResult::Connected { provider, model } => {
            {
                let mut g = ctx.lock().await;
                g.model = model.clone();
                g.system = Some(runtime.system_prompt.clone());
            }
            state.model = model.clone();
            state.chat.add_system(format!("Connected — switched to {provider}/{model}"));
        }
        ConnectResult::Rejected(msg) => state.chat.add_error(None, msg),
    }
}

/// 从 catalog 构造 `/connect` 向导（含末尾 custom 行）。
fn build_setup_modal(state: &AppState) -> ProviderSetupModal {
    let catalog = neko_providers::catalog::load(
        Some(&neko_core::session::paths::config_dir()),
        Some(std::path::Path::new(&state.cwd)),
    );
    let mut rows: Vec<ProviderRow> = catalog.iter().map(|(id, e)| ProviderRow {
        id:            id.clone(),
        name:          e.name.clone(),
        kind:          e.kind.clone(),
        base_url:      e.base_url.clone(),
        api_key_env:   e.api_key_env.clone(),
        default_model: e.default_model.clone(),
    }).collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows.push(ProviderRow::custom());
    ProviderSetupModal::new(rows)
}

#[allow(clippy::too_many_arguments)]
async fn handle_key(
    ke:       KeyEvent,
    runtime:  &mut BootstrappedRuntime,
    ctx:      &Arc<TokioMutex<AgentContext>>,
    state:    &mut AppState,
    history:  &mut History,
    perm_tx:  &mpsc::Sender<PermissionRequest>,
    done_tx:  &mpsc::Sender<TurnResult>,
    picker_tx: &mpsc::Sender<ModelPickerModal>,
) -> Control {
    let ctrl = ke.modifiers.contains(KeyModifiers::CONTROL);
    let alt  = ke.modifiers.contains(KeyModifiers::ALT);

    // 非 Ctrl+C 按键清除退出确认态
    if !(ctrl && ke.code == KeyCode::Char('c')) {
        state.exit_pending = false;
    }

    match ke.code {
        KeyCode::Char('c') if ctrl => {
            if state.is_running {
                if let Some(sig) = &state.signal {
                    sig.cancel();
                }
                state.status_msg = Some("cancelling…".into());
                // 5s safety net: force reset if agent never responds to cancel
                let done3 = done_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    let _ = done3.send(TurnResult::Cancelled).await;
                });
            } else if state.input.is_empty() {
                if state.exit_pending {
                    return Control::Quit;
                }
                state.exit_pending = true;
                state.status_msg = Some("Press Ctrl+C again to exit".into());
            } else {
                state.exit_pending = false;
                state.input.clear();
            }
        }
        KeyCode::Char('o') if ctrl => {
            state.think_show = !state.think_show;
            let msg = if state.think_show { "thinking: show" } else { "thinking: hide" };
            state.chat.add_system(msg);
        }
        KeyCode::Char('t') if ctrl => {
            // 循环 thinking budget: off → 2k → 4k → 8k → 16k → 32k
            state.think_enabled = !state.think_enabled;
            if state.think_enabled {
                state.think_budget = match state.think_budget {
                    b if b <= 2048 => 4096,
                    b if b <= 4096 => 8192,
                    b if b <= 8192 => 16384,
                    _              => 2048,
                };
            }
            let msg = if state.think_enabled {
                format!("thinking: ON ({} tokens)", state.think_budget)
            } else {
                "thinking: OFF".to_string()
            };
            state.chat.add_system(&msg);
        }
        KeyCode::Char('x') if ctrl => {
            // 更多选项：任务面板 + 状态信息
            state.refresh_tasks();
            state.show_tasks = !state.show_tasks;
            if state.show_tasks {
                state.chat.add_system("tasks: show");
            } else {
                state.chat.add_system("tasks: hide");
            }
        }
        // 某些终端/键盘把 Backspace 编码为 BS(0x08)，crossterm 解码为 Ctrl+H。
        KeyCode::Char('h') if ctrl => {
            state.input.backspace();
        }
        KeyCode::Tab => {
            if !state.suggestions.is_empty() {
                // 接受选中的命令补全
                let idx = state.suggestion_idx.min(state.suggestions.len() - 1);
                let val = state.suggestions[idx].value.clone();
                state.input.set(format!("{} ", val));
            } else {
                let next = match runtime.mode {
                    neko_core::ModeName::Ask   => neko_core::ModeName::Edit,
                    neko_core::ModeName::Edit  => neko_core::ModeName::Plan,
                    neko_core::ModeName::Plan  => neko_core::ModeName::Build,
                    neko_core::ModeName::Build => neko_core::ModeName::Agent,
                    neko_core::ModeName::Agent => neko_core::ModeName::Ask,
                };
                runtime.mode = next;
                runtime.permissions.lock().await.set_mode(next);
                state.mode = next.to_string();
            }
        }
        KeyCode::Esc => {
            if state.active_sub_agent.is_some() {
                // 从子 agent 视图切回主视图
                state.active_sub_agent = None;
            } else if state.is_running {
                if let Some(sig) = &state.signal {
                    sig.cancel();
                }
                // 中断循环
                if state.loop_state.is_some() {
                    state.loop_state = None;
                    state.chat.add_system("⟳ loop cancelled");
                }
            } else if !state.suggestions.is_empty() {
                // 关闭补全：清空当前 `/` 输入
                state.input.clear();
            } else if state.loop_state.is_some() {
                // 空闲时取消循环
                state.loop_state = None;
                state.chat.add_system("⟳ loop cancelled");
            }
        }
        KeyCode::Enter if alt => {
            state.input.insert_newline();
        }
        KeyCode::Enter => {
            // 补全可见且仍在敲命令名（无空格）且未敲完整 → Enter 先接受补全
            let accept_suggestion = !state.suggestions.is_empty()
                && !state.input.value.contains(char::is_whitespace)
                && {
                    let idx = state.suggestion_idx.min(state.suggestions.len() - 1);
                    state.suggestions[idx].value != state.input.value.trim()
                };

            // 已有完整命令（含空格）时清除建议，避免二次回车
            if !state.suggestions.is_empty() && state.input.value.contains(char::is_whitespace) {
                state.suggestions.clear();
            }

            if state.is_running {
                // 运行时压入队列，Agent 完成后自动消费
                let text = state.input.take();
                if !text.trim().is_empty() {
                    state.queued_messages.push(text);
                    state.status_msg = Some(format!("queued({})", state.queued_messages.len()));
                }
            } else if accept_suggestion {
                let idx = state.suggestion_idx.min(state.suggestions.len() - 1);
                let val = state.suggestions[idx].value.clone();
                state.suggestions.clear();
                let text = format!("{} ", val);
                state.input.clear();
                history.push(&text).await;
                history.reset_cursor();
                match handle_command(&text, runtime, ctx, state, picker_tx).await {
                    CmdResult::NotCommand => {}
                    CmdResult::Send(prompt) => {
                        start_turn(runtime, ctx, state, perm_tx, done_tx, prompt).await;
                    }
                    CmdResult::Handled => {}
                    CmdResult::Quit => return Control::Quit,
                }
            } else if !state.input.is_empty() {
                let text = state.input.take();
                history.push(&text).await;
                history.reset_cursor();

                // slash 命令处理
                match handle_command(&text, runtime, ctx, state, picker_tx).await {
                    CmdResult::NotCommand => {
                        start_turn(runtime, ctx, state, perm_tx, done_tx, text).await;
                    }
                    CmdResult::Send(prompt) => {
                        start_turn(runtime, ctx, state, perm_tx, done_tx, prompt).await;
                    }
                    CmdResult::Handled => {}
                    CmdResult::Quit => return Control::Quit,
                }
            }
        }
        // 仅插入无修饰键的可见字符；任何 Ctrl/Alt 组合（如 Ctrl+H/Ctrl+A）都不应漏插字母。
        KeyCode::Char(c) if !ctrl && !alt => {
            state.input.insert_char(c);
        }
        KeyCode::Backspace => {
            state.input.backspace();
        }
        KeyCode::Delete => {
            state.input.delete_forward();
        }
        KeyCode::Left => {
            state.input.move_left();
        }
        KeyCode::Right => {
            state.input.move_right();
        }
        KeyCode::Home => {
            state.input.move_home();
        }
        KeyCode::End => {
            state.input.move_end();
        }
        KeyCode::Up if alt => { state.chat.scroll_up(3); }
        KeyCode::Down if alt => { state.chat.scroll_down(3); }
        // Ctrl+↑↓ 切换子 agent 视图
        KeyCode::Up if ctrl => {
            let ids = sub_agent_ids(&state.chat);
            if !ids.is_empty() {
                let next = match state.active_sub_agent {
                    None => Some(ids[ids.len() - 1]),
                    Some(id) => {
                        if let Some(pos) = ids.iter().position(|x| *x == id) {
                            if pos > 0 { Some(ids[pos - 1]) } else { None }
                        } else {
                            Some(ids[ids.len() - 1])
                        }
                    }
                };
                state.active_sub_agent = next;
            }
        }
        KeyCode::Down if ctrl => {
            let ids = sub_agent_ids(&state.chat);
            if !ids.is_empty() {
                let next = match state.active_sub_agent {
                    None => Some(ids[0]),
                    Some(id) => {
                        if let Some(pos) = ids.iter().position(|x| *x == id) {
                            if pos + 1 < ids.len() { Some(ids[pos + 1]) } else { None }
                        } else {
                            Some(ids[0])
                        }
                    }
                };
                state.active_sub_agent = next;
            }
        }
        KeyCode::Up => {
            if !state.suggestions.is_empty() {
                let n = state.suggestions.len();
                state.suggestion_idx = (state.suggestion_idx + n - 1) % n;
            } else if !state.input.is_empty() {
                // 输入框有内容：多行光标上移
                state.input.move_up();
            } else if let Some(prev) = history.prev() {
                state.input.set(prev);
            }
        }
        KeyCode::Down => {
            if !state.suggestions.is_empty() {
                let n = state.suggestions.len();
                state.suggestion_idx = (state.suggestion_idx + 1) % n;
            } else if !state.input.is_empty() {
                // 输入框有内容：多行光标下移
                state.input.move_down();
            } else if let Some(next) = history.next() {
                state.input.set(next);
            }
        }
        KeyCode::PageUp => { state.chat.scroll_up(10); }
        KeyCode::PageDown => { state.chat.scroll_down(10); }
        _ => {}
    }

    // 输入变化后重算补全：光标处有 `@token` → 文件补全；否则 `/` 命令补全。
    let cwd = std::path::Path::new(&state.cwd);
    state.suggestions = crate::repl::file_complete::maybe_suggestions(
        &state.input.value, state.input.cursor_pos, cwd,
    ).unwrap_or_else(|| {
        crate::repl::commands::command_suggestions(&state.input.value, &runtime.skills)
    });
    if state.suggestion_idx >= state.suggestions.len() {
        state.suggestion_idx = 0;
    }

    Control::Continue
}

/// 命令处理结果。
enum CmdResult {
    /// 不是命令，原样作为 prompt 发送
    NotCommand,
    /// 已处理，无需发送
    Handled,
    /// 退出
    Quit,
    /// 发送指定 prompt（用于技能展开）
    Send(String),
}

/// 处理 slash 命令。
async fn handle_command(
    text:      &str,
    runtime:   &mut BootstrappedRuntime,
    ctx:       &Arc<TokioMutex<AgentContext>>,
    state:     &mut AppState,
    picker_tx: &mpsc::Sender<ModelPickerModal>,
) -> CmdResult {
    use crate::repl::commands::{handle, CommandOutcome};

    if !text.trim().starts_with('/') {
        return CmdResult::NotCommand;
    }

    match handle(text, &runtime.skills) {
        CommandOutcome::NotACommand(_) => CmdResult::NotCommand,
        CommandOutcome::RunSkill { prompt } => {
            state.chat.add_system(format!("running skill: {}", text.trim()));
            CmdResult::Send(prompt)
        }
        CommandOutcome::SwitchMode(mode) => {
            runtime.mode = mode;
            runtime.permissions.lock().await.set_mode(mode);
            state.mode = mode.to_string();
            state.chat.add_system(format!("mode switched to {}", mode));
            CmdResult::Handled
        }
        CommandOutcome::OpenModePicker => {
            state.mode_picker = Some(ModePickerModal::new(runtime.mode));
            CmdResult::Handled
        }
        CommandOutcome::SwitchModel(model_ref) => {
            switch_model(runtime, ctx, state, model_ref).await;
            CmdResult::Handled
        }
        CommandOutcome::OpenProviderSetup => {
            state.provider_setup = Some(build_setup_modal(state));
            CmdResult::Handled
        }
        CommandOutcome::QuickConnect { provider, api_key, base_url } => {
            quick_connect(runtime, ctx, state, provider, api_key, base_url).await;
            CmdResult::Handled
        }
        CommandOutcome::OpenModelPicker => {
            use super::widgets::model_picker::ModelEntry;

            let active_provider = runtime.provider.as_ref().map(|p| p.id().to_string());
            let active_model = state.model.clone();

            // 收集所有已注册 provider 的缓存模型，跨 provider 分组展示。
            let mut entries = Vec::new();
            for p in runtime.provider_registry.list() {
                let pid = p.id().to_string();
                let pname = p.display_name().to_string();
                if let Some(ids) = runtime.config.models.get(&pid) {
                    for id in ids {
                        entries.push(ModelEntry {
                            id:            id.clone(),
                            display_name:  String::new(),
                            provider_id:   pid.clone(),
                            provider_name: pname.clone(),
                        });
                    }
                }
            }

            if entries.is_empty() {
                // 无任何缓存 → 后台拉取当前 provider 模型，拉完回传 picker。
                let Some(provider) = runtime.provider.clone() else {
                    state.chat.add_system("No provider configured — run /connect first.");
                    return CmdResult::Handled;
                };
                let pid = provider.id().to_string();
                let pname = provider.display_name().to_string();
                let tx = picker_tx.clone();
                state.status_msg = Some("loading models…".into());
                tokio::spawn(async move {
                    let models = provider.list_models().await.unwrap_or_default();
                    let ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
                    crate::connect::cache_models(&pid, &ids).await;
                    let entries: Vec<ModelEntry> = models
                        .into_iter()
                        .map(|m| ModelEntry {
                            id:            m.id,
                            display_name:  m.display_name,
                            provider_id:   pid.clone(),
                            provider_name: pname.clone(),
                        })
                        .collect();
                    if !entries.is_empty() {
                        let _ = tx
                            .send(ModelPickerModal::new(entries, pid, active_model))
                            .await;
                    }
                });
                return CmdResult::Handled;
            }

            state.model_picker = Some(ModelPickerModal::new(
                entries,
                active_provider.unwrap_or_default(),
                active_model,
            ));

            // 后台静默刷新当前 provider 的磁盘缓存。
            if let Some(provider) = runtime.provider.clone() {
                let pid = provider.id().to_string();
                tokio::spawn(async move {
                    let ids: Vec<String> = provider
                        .list_models()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|m| m.id)
                        .collect();
                    crate::connect::cache_models(&pid, &ids).await;
                });
            }
            CmdResult::Handled
        }
        CommandOutcome::SwitchThinking { enabled, budget } => {
            state.think_enabled = enabled;
            if let Some(b) = budget { state.think_budget = b; }
            if state.think_enabled {
                state.chat.add_system(format!(
                    "thinking ON (budget: {} tokens)", state.think_budget
                ));
            } else {
                state.chat.add_system("thinking OFF");
            }
            CmdResult::Handled
        }
        CommandOutcome::ToggleThinkingDisplay => {
            state.think_show = !state.think_show;
            state.chat.add_system(format!(
                "reasoning display: {}", if state.think_show { "expanded" } else { "folded" }
            ));
            CmdResult::Handled
        }
        CommandOutcome::SetEffort(level) => {
            if level == "off" {
                state.effort = None;
                state.chat.add_system("effort: off (model default)");
            } else {
                state.effort = Some(level.clone());
                state.chat.add_system(format!("effort: {level}"));
            }
            CmdResult::Handled
        }
        CommandOutcome::NewSession => {
            let new_session = neko_core::session::create_session(
                runtime.session.meta.cwd.clone(),
                Some(runtime.model.clone()),
            ).await;
            let messages = {
                let mut guard = ctx.lock().await;
                let new_ctx = neko_engine::AgentContext::from_session(
                    &new_session,
                    runtime.model.clone(),
                    Some(runtime.system_prompt.clone()),
                );
                *guard = new_ctx;
                guard.messages.clone()
            };
            runtime.session = new_session;
            state.chat = ChatWidget::new();
            state.load_history_messages(&messages);
            state.chat.add_system("Started new conversation");
            CmdResult::Handled
        }
        CommandOutcome::Clear => {
            state.chat = ChatWidget::new();
            CmdResult::Handled
        }
        CommandOutcome::Compact => {
            compact_tui(runtime, ctx, state).await;
            CmdResult::Handled
        }
        CommandOutcome::Resume(id) => {
            resume_session_tui(runtime, ctx, state, id).await;
            CmdResult::Handled
        }
        CommandOutcome::OpenSessionPicker => {
            let sessions = neko_core::session::list_sessions().await;
            let entries: Vec<SessionEntry> = sessions.into_iter().map(|s| {
                let when = chrono::DateTime::from_timestamp_millis(s.updated_at)
                    .map(|d: chrono::DateTime<chrono::Utc>| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".to_string());
                SessionEntry {
                    id: s.id,
                    title: s.title.unwrap_or_default(),
                    message_count: s.message_count,
                    updated_at: when,
                }
            }).collect();
            state.session_picker = Some(SessionPickerModal::new(entries));
            CmdResult::Handled
        }
        CommandOutcome::Quit => CmdResult::Quit,
        CommandOutcome::EnterPlan(desc) => {
            let prompt = format!(
                "The user wants to create a plan. Task: {}\n\n\
                 Use `enter_plan_mode` to switch to plan mode, research, \
                 write the plan, then call `exit_plan_mode` to submit.",
                if desc.is_empty() { "architecture / task planning" } else { &desc }
            );
            state.chat.add_system("entering plan mode…");
            CmdResult::Send(prompt)
        }
        CommandOutcome::InitAgentsMd => {
            let agents_path = runtime.cwd.join("AGENTS.md");
            if agents_path.exists() {
                state.chat.add_system(format!(
                    "AGENTS.md already exists at {}. Ask neko to update it, or delete it first.",
                    agents_path.display()
                ));
                return CmdResult::Handled;
            }
            let prompt = build_init_prompt(&runtime.cwd);
            state.chat.add_system("generating AGENTS.md…");
            CmdResult::Send(prompt)
        }
        CommandOutcome::LoopStart { goal, max_turns } => {
            let ls = neko_core::session::loop_state::LoopState::new(goal.clone(), max_turns);
            state.chat.add_system(format!(
                "⟳ loop started: {} (max {} turns — esc to stop)",
                goal, max_turns
            ));
            state.loop_state = Some(ls);
            // goal 作为第一轮 prompt，start_turn 会自动追加 loop snippet
            CmdResult::Send(goal)
        }
        CommandOutcome::LoopStop => {
            if let Some(ls) = state.loop_state.take() {
                state.chat.add_system(format!(
                    "⟳ loop stopped (turn {}/{})",
                    ls.current_turn, ls.max_turns
                ));
            } else {
                state.chat.add_system("no active loop");
            }
            CmdResult::Handled
        }
        CommandOutcome::LoopStatus => {
            if let Some(ls) = &state.loop_state {
                state.chat.add_system(format!(
                    "⟳ loop: turn {}/{}, goal: {}",
                    ls.current_turn, ls.max_turns, ls.goal
                ));
            } else {
                state.chat.add_system("no active loop");
            }
            CmdResult::Handled
        }
        CommandOutcome::Reload => {
            let cwd = std::path::PathBuf::from(&state.cwd);
            // 1. 重载配置
            let resolved = neko_core::load_config(Some(&cwd)).await;
            let provider_count = resolved.providers.len();
            // 2. 重建 provider 注册表
            let boot = neko_providers::build_registry(&resolved);
            let new_registry = std::sync::Arc::new(boot.registry);
            // 3. 保持当前 provider（如果还在注册表中）
            let current_pid = runtime.provider.as_ref().map(|p| p.id().to_string());
            runtime.provider_registry = new_registry;
            if let Some(pid) = current_pid {
                runtime.provider = runtime.provider_registry.get(&pid);
            }
            // 4. 更新配置 + 重建 catalog + system prompt
            runtime.config = resolved;
            runtime.rebuild_context().await;
            // 5. 重载 skills
            let mut skills = neko_core::skills::SkillRegistry::new();
            neko_skills::load_builtin_skills(&mut skills);
            let global_dir = neko_core::session::paths::skills_dir();
            neko_skills::load_skills_from_dir(&mut skills, &global_dir).await;
            neko_skills::load_skills_from_dir(&mut skills, &cwd.join(".agents").join("skills")).await;
            neko_skills::load_skills_from_dir(&mut skills, &cwd.join(".neko").join("skills")).await;
            let skill_count = skills.list().len();
            runtime.skills = std::sync::Arc::new(skills);
            state.chat.add_system(format!(
                "⟳ reloaded: {provider_count} provider(s), {skill_count} skill(s)"
            ));
            CmdResult::Handled
        }
        CommandOutcome::Handled => {
            let trimmed = text.trim();
            if let Some(rest) = trimmed.strip_prefix("/memory").or_else(|| trimmed.strip_prefix("/mem")) {
                let rest = rest.trim();
                let entries = if let Some(q) = rest.strip_prefix("search").map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    neko_core::search_memory(q).await
                } else {
                    neko_core::list_memory().await
                };
                if entries.is_empty() {
                    state.chat.add_system("(no memories)");
                } else {
                    for e in &entries {
                        state.chat.add_system(format!("[{:?}] {} — {}", e.memory_type, e.title, e.body));
                    }
                }
            }
            CmdResult::Handled
        }
    }
}

async fn switch_model(
    runtime:   &mut BootstrappedRuntime,
    ctx:       &Arc<TokioMutex<AgentContext>>,
    state:     &mut AppState,
    model_ref: String,
) {
    use crate::connect::SwitchResult;
    match crate::connect::switch_model(runtime, &model_ref).await {
        SwitchResult::Switched { provider, model } => {
            {
                let mut g = ctx.lock().await;
                g.model = model.clone();
                g.system = Some(runtime.system_prompt.clone());
            }
            state.model = model.clone();
            state.chat.add_system(format!("switched to {provider}/{model}"));
        }
        SwitchResult::ModelOnly { model } => {
            {
                let mut g = ctx.lock().await;
                g.model = model.clone();
                g.system = Some(runtime.system_prompt.clone());
            }
            state.model = model.clone();
            state.chat.add_system(format!("model switched to {model}"));
        }
        SwitchResult::ProviderMissing { provider } => {
            state.chat.add_error(None, format!("provider '{provider}' not available"));
        }
        SwitchResult::NoProvider => {
            state.chat.add_system("No provider configured — run /connect first.");
        }
    }
}

/// 发起一轮 agent 执行（异步任务）。
async fn start_turn(
    runtime: &BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
    perm_tx: &mpsc::Sender<PermissionRequest>,
    done_tx: &mpsc::Sender<TurnResult>,
    text:    String,
) {
    // 未配置 provider：拦下消息，提示并打开 /connect 向导（对照 hermes setup-required 门控）。
    let Some(provider) = runtime.provider.clone() else {
        state.chat.add_system("No provider configured — run /connect to add one before sending messages.");
        if state.provider_setup.is_none() {
            state.provider_setup = Some(build_setup_modal(state));
        }
        return;
    };

    // 聊天区显示用户原文（保留 `@path` token）；发给模型的消息追加被引用文件的内容。
    state.chat.add_user(text.clone());
    let attachments = crate::repl::file_complete::expand_mentions(&text, std::path::Path::new(&state.cwd));
    let model_text = if attachments.is_empty() { text } else { format!("{text}{attachments}") };

    // 添加用户消息并持久化
    let session_id = runtime.session.meta.id;
    {
        let msg = Message::user_text(model_text);
        let mut guard = ctx.lock().await;
        guard.add_message(msg.clone());
        drop(guard);
        session::append_message(session_id, msg).await.ok();
    }

    let signal = CancellationToken::new();
    state.signal = Some(signal.clone());
    state.is_running = true;
    state.is_thinking = true;
    state.status_msg = None;
    state.turn_start_ms = Some(chrono::Utc::now().timestamp_millis());

    let mut executor = neko_engine::agent::orchestrator::build_executor(
        runtime.tools.clone(),
        runtime.permissions.clone(),
        runtime.bus.clone(),
        runtime.catalog.clone(),
        runtime.model.clone(),
        runtime.session.meta.id,
        runtime.cwd.clone(),
        neko_providers::provider::DEFAULT_MAX_OUTPUT_TOKENS as u64,
        provider,
        Some(perm_tx.clone()),
    );
    executor.thinking_budget = if state.think_enabled { Some(state.think_budget) } else { None };
    executor.reasoning_effort = state.effort.clone();

    let ctx2 = ctx.clone();
    let done2 = done_tx.clone();
    // 构建 system prompt（循环模式追加 loop snippet）
    let sys_prompt = if let Some(ls) = &state.loop_state {
        format!("{}\n\n{}", runtime.system_prompt, ls.build_system_prompt_snippet())
    } else {
        runtime.system_prompt.clone()
    };
    let handle = tokio::spawn(async move {
        let mut guard = ctx2.lock().await;
        guard.system = Some(sys_prompt);
        executor.run(&mut guard, signal).await
    });
    tokio::spawn(async move {
        let result = match handle.await {
            Ok(r) => r,
            Err(e) => TurnResult::Error(format!("agent task panicked: {}", e)),
        };
        let _ = done2.send(result).await;
    });
}

fn finish_turn(state: &mut AppState, result: TurnResult) {
    state.is_running  = false;
    state.is_thinking = false;
    state.signal = None;
    state.chat.end_turn();
    state.turn_start_ms = None;
    state.refresh_tasks();

    match result {
        TurnResult::Done { .. } | TurnResult::Continue => {}
        TurnResult::MaxTurns => state.chat.add_system("reached max turns limit"),
        TurnResult::Cancelled => state.chat.add_system("cancelled"),
        TurnResult::Error(e) => state.chat.add_error(None, e),
    }
    state.status_msg = None;
}

/// 恢复指定会话：加载消息、替换 chat 气泡与运行时会话。
async fn resume_session_tui(
    runtime: &mut BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
    id:      uuid::Uuid,
) {
    match neko_core::session::load_session(id).await {
        Some(s) => {
            let messages = {
                let mut guard = ctx.lock().await;
                let new_ctx = neko_engine::AgentContext::from_session(
                    &s,
                    runtime.model.clone(),
                    Some(runtime.system_prompt.clone()),
                );
                *guard = new_ctx;
                guard.messages.clone()
            };
            runtime.session = s;
            state.chat = ChatWidget::new();
            state.load_history_messages(&messages);
            state.chat.add_system(format!(
                "Resumed session {} ({} messages)", id, messages.len()
            ));
        }
        None => {
            state.chat.add_system(format!("Session {} not found", id));
        }
    }
}

/// 删除会话：从列表移除并删除文件。
async fn delete_session_tui(state: &mut AppState, id: uuid::Uuid) {
    if let Err(e) = neko_core::session::delete_session(id).await {
        state.chat.add_system(format!("delete failed: {e}"));
        return;
    }
    // 从 picker 列表移除
    let label = if let Some(picker) = &mut state.session_picker {
        let title = picker.selected_title().unwrap_or_default().to_string();
        picker.remove_selected();
        title
    } else {
        id.to_string()
    };
    state.chat.add_system(format!("deleted: {label}"));
}

/// 重命名会话。
async fn rename_session_tui(state: &mut AppState, id: uuid::Uuid, old_name: &str) {
    // 从列表中取当前 title 作为默认值
    let default = if old_name.is_empty() { id.to_string() } else { old_name.to_string() };
    state.rename_input = Some(super::widgets::session_picker::RenameState {
        session_id: id,
        current_title: default,
    });
}

/// Fork 会话：复制到新会话并立即切换。
async fn fork_session_tui(
    runtime: &mut BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
    id:      uuid::Uuid,
) {
    match neko_core::session::fork_session(id).await {
        Ok(new_session) => {
            let new_id = new_session.meta.id;
            // 立即切换到新会话
            let messages = {
                let mut guard = ctx.lock().await;
                let new_ctx = neko_engine::AgentContext::from_session(
                    &new_session,
                    runtime.model.clone(),
                    Some(runtime.system_prompt.clone()),
                );
                *guard = new_ctx;
                guard.messages.clone()
            };
            runtime.session = new_session;
            state.chat = ChatWidget::new();
            state.load_history_messages(&messages);
            state.chat.add_system(format!(
                "forked {} → {} ({} messages)",
                id, new_id, messages.len()
            ));
            // 刷新 picker 列表
            refresh_session_picker(state).await;
        }
        Err(e) => {
            state.chat.add_system(format!("fork failed: {e}"));
        }
    }
}

/// 刷新会话 picker 列表。
async fn refresh_session_picker(state: &mut AppState) {
    use super::widgets::session_picker::SessionEntry;
    let sessions = neko_core::session::list_sessions().await;
    let entries: Vec<SessionEntry> = sessions.into_iter().map(|s| {
        let when = chrono::DateTime::from_timestamp_millis(s.updated_at)
            .map(|d: chrono::DateTime<chrono::Utc>| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_string());
        SessionEntry {
            id: s.id,
            title: s.title.unwrap_or_default(),
            message_count: s.message_count,
            updated_at: when,
        }
    }).collect();
    state.session_picker = Some(super::widgets::session_picker::SessionPickerModal::new(entries));
}

fn draw<B: ratatui::backend::Backend>(term: &mut Terminal<B>, state: &mut AppState) -> Result<()> {
    term.draw(|frame| {
        let area = frame.area();
        let input_lines = state.input.visual_line_count(area.width);
        let base = AppLayout::compute(area, input_lines);

        // ── 确定 footer zone 四态（审核 > 选择 > 通知 > 提醒）──────────────────
        let show_suggestions = !state.suggestions.is_empty() && state.pending.is_none()
            && state.session_picker.is_none() && state.model_picker.is_none()
            && state.mode_picker.is_none() && state.provider_setup.is_none();

        let token_pct = if state.show_token_count && state.tokens > 0 && state.context_window > 0 {
            Some(state.tokens as f64 / state.context_window as f64 * 100.0)
        } else {
            None
        };
        let has_notification = state.status_msg.is_some()
            || token_pct.map_or(false, |p| p >= 70.0);

        let zone_h: u16 = if let Some(p) = &state.pending {
            p.modal.height()
        } else if let Some(pk) = &state.model_picker {
            pk.height()
        } else if let Some(pk) = &state.mode_picker {
            pk.height()
        } else if let Some(pk) = &state.session_picker {
            pk.height()
        } else if let Some(s) = &state.provider_setup {
            s.height()
        } else if show_suggestions {
            suggestions::height(state.suggestions.len())
        } else {
            1 // 通知 / 提醒：固定 1 行
        };

        // ── 布局：chat / input / footer_zone / status_bar ─────────────────────
        let input_h = base.input.height;
        let status_h = 1u16;
        let avail = area.height.saturating_sub(input_h + status_h + 1).max(1);
        let zh = zone_h.min(avail);

        use ratatui::layout::{Constraint, Direction, Layout};
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),           // chat（收缩让位）
                Constraint::Length(input_h),  // input
                Constraint::Length(zh),       // footer zone（四态切换）
                Constraint::Length(status_h), // status bar（固定）
            ])
            .split(area);

        let chat_area = chunks[0];
        let input_area = chunks[1];
        let zone_area = chunks[2];
        let status_area = chunks[3];

        // chat area
        let show_welcome = !state.chat.has_conversation();
        if show_welcome {
            frame.render_widget(
                render_welcome(&state.model, &state.mode, &state.cwd),
                chat_area,
            );
        } else {
            let chat_widget = state.chat.render(chat_area, state.think_show, state.active_sub_agent);
            frame.render_widget(chat_widget, chat_area);
        }

        // input box
        let ghost = crate::repl::commands::inline_ghost(&state.input.value, &state.suggestions);
        let arg_hint = crate::repl::commands::argument_hint(&state.input.value);
        frame.render_widget(state.input.render(&state.mode, &ghost, arg_hint, input_area.width), input_area);

        // ── footer zone 渲染（四态）──────────────────────────────────────────
        frame.render_widget(ratatui::widgets::Clear, zone_area);
        if let Some(p) = &state.pending {
            frame.render_widget(p.modal.render(), zone_area);
        } else if let Some(picker) = &state.model_picker {
            frame.render_widget(picker.render(), zone_area);
        } else if let Some(picker) = &state.mode_picker {
            frame.render_widget(picker.render(), zone_area);
        } else if let Some(picker) = &state.session_picker {
            frame.render_widget(picker.render(state.rename_input.as_ref(), state.session_action.as_ref()), zone_area);
        } else if let Some(setup) = &state.provider_setup {
            frame.render_widget(setup.render(), zone_area);
        } else if show_suggestions {
            frame.render_widget(
                suggestions::render(&state.suggestions, state.suggestion_idx),
                zone_area,
            );
        } else if has_notification {
            frame.render_widget(render_notification(state, token_pct), zone_area);
        } else {
            frame.render_widget(render_hint(state), zone_area);
        }

        // ── status bar（固定 1 行）───────────────────────────────────────────
        frame.render_widget(render_status_bar(state, status_area.width, token_pct), status_area);

        // cursor（picker/向导/权限激活时隐藏；仅 suggestions 时仍在输入框内）
        if state.pending.is_none() && state.session_picker.is_none()
            && state.model_picker.is_none() && state.mode_picker.is_none()
            && state.provider_setup.is_none()
        {
            let (cx, cy) = state.input.cursor_screen_pos(input_area);
            frame.set_cursor_position((cx, cy));
        }

        // 任务面板（Ctrl+T）：无下拉菜单时锚定输入框上方
        let no_menu = state.pending.is_none() && state.model_picker.is_none()
            && state.mode_picker.is_none()
            && state.session_picker.is_none() && state.provider_setup.is_none()
            && !show_suggestions;
        if no_menu && state.show_tasks && !state.tasks.is_empty() {
            let panel_area = tasks::area(area, base.input.y, state.tasks.len());
            frame.render_widget(ratatui::widgets::Clear, panel_area);
            frame.render_widget(tasks::render(&state.tasks), panel_area);
        }
    }).map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(())
}

// ── footer zone 渲染函数 ──────────────────────────────────────────────────────

/// 通知态：显示临时状态消息或 token 警告。
fn render_notification(state: &AppState, token_pct: Option<f64>) -> ratatui::widgets::Paragraph<'static> {
    let dim = ratatui::style::Style::default().fg(MUTED);

    if let Some(ref msg) = state.status_msg {
        return ratatui::widgets::Paragraph::new(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled("● ", ratatui::style::Style::default().fg(WARN)),
            ratatui::text::Span::styled(msg.clone(), dim),
        ]));
    }

    if let Some(pct) = token_pct {
        if pct >= 85.0 {
            return ratatui::widgets::Paragraph::new(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled("● ", ratatui::style::Style::default().fg(ERR)),
                ratatui::text::Span::styled(
                    format!("Context low ({:.0}% remaining) · /compact to summarize", 100.0 - pct),
                    ratatui::style::Style::default().fg(ERR),
                ),
            ]));
        } else if pct >= 70.0 {
            return ratatui::widgets::Paragraph::new(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled("● ", ratatui::style::Style::default().fg(WARN)),
                ratatui::text::Span::styled(
                    format!("{:.0}% context used · /compact to free space", pct),
                    dim,
                ),
            ]));
        }
    }

    ratatui::widgets::Paragraph::new(ratatui::text::Line::from(""))
}

/// 提醒态：上下文感知的快捷键提示。
fn render_hint(state: &AppState) -> ratatui::widgets::Paragraph<'static> {
    let dim = ratatui::style::Style::default().fg(MUTED);
    let accent = ratatui::style::Style::default().fg(THINK);

    let sub_ids = sub_agent_ids(&state.chat);

    let hint = if let Some(id) = state.active_sub_agent {
        // 子 agent 视图
        format!("⟳ sub-agent {} · ↓ cycle · Esc back to main", &id.to_string()[..8])
    } else if let Some(ls) = &state.loop_state {
        // 循环模式
        format!("⟳ loop {}/{} · {} · esc to stop", ls.current_turn, ls.max_turns, ls.goal)
    } else if state.is_running {
        if !sub_ids.is_empty() {
            format!("{} agent(s) active · ↓ to view · esc to interrupt", sub_ids.len())
        } else {
            "esc to interrupt".to_string()
        }
    } else if !state.queued_messages.is_empty() {
        format!("{} queued · Enter to send", state.queued_messages.len())
    } else if !sub_ids.is_empty() {
        format!("{} sub-agent(s) · ↓ to review · ? help · / commands · @ files", sub_ids.len())
    } else {
        "? help · Tab mode · / commands · @ files".to_string()
    };

    let style = if state.active_sub_agent.is_some() || state.loop_state.is_some() { accent } else { dim };
    ratatui::widgets::Paragraph::new(ratatui::text::Line::from(
        ratatui::text::Span::styled(hint, style),
    ))
}

/// 状态栏：模式 · 模型 · token · 右侧运行状态。
fn render_status_bar(state: &AppState, max_w: u16, token_pct: Option<f64>) -> ratatui::widgets::Paragraph<'static> {
    use unicode_width::UnicodeWidthStr;

    let dim = ratatui::style::Style::default().fg(MUTED);
    let bold_dim = dim.add_modifier(ratatui::style::Modifier::BOLD);
    let mcolor = super::theme::mode_color(&state.mode);
    let max_w = max_w as usize;

    let mut spans: Vec<ratatui::text::Span<'static>> = Vec::new();

    // 模式
    spans.push(ratatui::text::Span::styled(
        format!("⏵⏵ {}", state.mode),
        bold_dim.fg(mcolor),
    ));
    spans.push(ratatui::text::Span::styled(" · ".to_string(), dim));

    // 模型
    spans.push(ratatui::text::Span::styled(state.model.clone(), dim));

    // spinner（紧跟模型名，保证始终可见）
    if state.is_running {
        let spin = SPINNER[state.spinner_idx % SPINNER.len()];
        spans.push(ratatui::text::Span::styled(format!(" · {} working", spin), dim));
    }

    // skip-perm
    if state.skip_perms {
        spans.push(ratatui::text::Span::styled(" · skip-perm".to_string(), dim.fg(ERR)));
    }

    // token 计数
    if let Some(pct) = token_pct {
        let tc = if pct >= 85.0 { ERR } else if pct >= 70.0 { WARN } else { MUTED };
        let tokens_k = state.tokens as f64 / 1000.0;
        let token_str = if tokens_k >= 1000.0 {
            format!("{:.1}M", tokens_k / 1000.0)
        } else {
            format!("{:.0}k", tokens_k)
        };
        spans.push(ratatui::text::Span::styled(
            format!(" · {}({:.0}%)", token_str, pct),
            dim.fg(tc),
        ));
    }

    // 右侧
    let mut right = String::new();
    let (_, in_prog_t, _) = tasks::counts(&state.tasks);
    if in_prog_t > 0 {
        right.push_str(&format!(" ◼{}", in_prog_t));
    }
    if !state.queued_messages.is_empty() {
        right.push_str(&format!(" queued({})", state.queued_messages.len()));
    }
    if right.is_empty() {
        right.push_str("Tab ↻ mode");
    }

    let right_w = right.width();
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    if used + right_w + 2 <= max_w {
        let pad = max_w - used - right_w;
        spans.push(ratatui::text::Span::raw(" ".repeat(pad)));
        spans.push(ratatui::text::Span::styled(right, dim));
    }

    ratatui::widgets::Paragraph::new(ratatui::text::Line::from(spans))
}

fn build_init_prompt(cwd: &std::path::Path) -> String {
    crate::repl::commands::build_init_prompt(cwd)
}

/// TUI 版会话压缩：直接调 provider 生成摘要，替换消息历史。
async fn compact_tui(
    runtime: &BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
) {
    use neko_core::tools::ContentBlock;
    use neko_providers::provider::ChatRequest;
    use crate::repl::commands::{
        COMPACT_SYSTEM_PROMPT, render_compact_transcript,
        split_for_compact, build_compact_message,
    };

    let (messages, session_id) = {
        let guard = ctx.lock().await;
        (guard.messages.clone(), runtime.session.meta.id)
    };

    let (prior_summary, new_messages) = split_for_compact(&messages);

    if new_messages.len() < 4 {
        state.chat.add_system("nothing to compact (conversation too short)");
        return;
    }

    let Some(provider) = runtime.provider.clone() else {
        state.chat.add_system("no provider configured — run /connect first");
        return;
    };

    state.chat.add_system("compacting conversation…");

    let transcript = render_compact_transcript(new_messages);
    let mut req = ChatRequest::new(runtime.model.clone(), vec![Message::user_text(transcript)]);
    req.system      = Some(COMPACT_SYSTEM_PROMPT.to_string());
    req.temperature = Some(0.3);
    req.max_tokens  = 4096;

    let signal = CancellationToken::new();
    match provider.chat(&req, signal).await {
        Ok(resp) => {
            let summary = resp.message.content.iter()
                .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                .collect::<Vec<_>>().join("\n");
            let summary = summary.trim().to_string();
            if summary.is_empty() {
                state.chat.add_system("compact: model returned empty summary, keeping full history");
                return;
            }
            let n = messages.len();
            let summary_msgs = build_compact_message(prior_summary.as_deref(), &summary);
            {
                let mut guard = ctx.lock().await;
                guard.replace_messages(summary_msgs.clone());
            }
            session::replace_messages(session_id, &summary_msgs).await.ok();
            state.chat.add_system(&format!(
                "compacted: {} messages → summary ({} chars)",
                n, summary.len()
            ));
        }
        Err(e) => {
            state.chat.add_system(&format!("compact failed: {e}"));
        }
    }
}
