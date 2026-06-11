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

use crate::agent::{
    AgentContext, PermissionDecision, PermissionRequest, TurnResult,
};
use crate::agent::tool_preview::extract_tool_preview;
use crate::args::Args;
use crate::bootstrap::BootstrappedRuntime;
use crate::repl::history::History;

use super::layout::AppLayout;
use super::widgets::{
    chat::ChatWidget,
    input::InputWidget,
    model_picker::ModelPickerModal,
    permission::PermissionModal,
    provider_setup::{ProviderRow, ProviderSetupModal, SetupAction},
    session_picker::{SessionEntry, SessionPickerModal},
    suggestions,
    tasks,
    welcome::render_welcome,
};
use super::theme::{SPINNER, MUTED, ERR, WARN};

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
    /// 运行时用户输入的消息队列（Agent 完成后续按顺序消费）
    queued_messages:  Vec<String>,
    /// Ctrl+C 首次按下后进入 pending 态，再次按下才退出
    exit_pending:     bool,
    /// 当前选中的子 agent 视图（None = 主 agent）
    active_sub_agent: Option<uuid::Uuid>,
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
            queued_messages:  Vec::new(),
            exit_pending:     false,
            active_sub_agent: None,
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
    let (setup_tx, mut setup_rx) = mpsc::channel::<Result<Vec<String>, String>>(1);

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
                // 消费队列：一轮完成后自动处理下一条排队消息
                if !state.queued_messages.is_empty() {
                    let text = state.queued_messages.remove(0);
                    start_turn(runtime, &ctx, &mut state, &perm_tx, &done_tx, text).await;
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
                // /connect 向导：模型拉取结果回传
                match result {
                    Ok(models) => {
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
    setup_tx:  &mpsc::Sender<Result<Vec<String>, String>>,
) -> Control {
    match ev {
        Event::Paste(text) => {
            if state.pending.is_none() && state.model_picker.is_none()
                && state.provider_setup.is_none()
            {
                state.input.insert_str(&text);
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
            if state.session_picker.is_some() {
                return handle_session_picker_key(ke, runtime, ctx, state).await;
            }
            // 模型选择器优先处理按键
            if state.model_picker.is_some() {
                return handle_model_picker_key(ke, runtime, ctx, state).await;
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
    match ke.code {
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
        KeyCode::Enter => {
            if let Some(picker) = state.model_picker.take() {
                if let Some(model_id) = picker.selected_id() {
                    let model_ref = format!("{}/{}", picker.provider, model_id);
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

/// 会话选择器按键处理。
async fn handle_session_picker_key(
    ke:      KeyEvent,
    runtime: &mut BootstrappedRuntime,
    ctx:     &Arc<TokioMutex<AgentContext>>,
    state:   &mut AppState,
) -> Control {
    match ke.code {
        KeyCode::Up => {
            if let Some(picker) = &mut state.session_picker {
                picker.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(picker) = &mut state.session_picker {
                picker.move_down();
            }
        }
        KeyCode::Enter => {
            if let Some(picker) = state.session_picker.take() {
                if let Some(id) = picker.selected_id() {
                    resume_session_tui(runtime, ctx, state, id).await;
                }
            }
        }
        KeyCode::Esc => {
            state.session_picker = None;
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
    setup_tx: &mpsc::Sender<Result<Vec<String>, String>>,
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
        SetupAction::Commit => finish_setup(runtime, ctx, state).await,
    }
    Control::Continue
}

/// 后台构造临时 provider 拉取 `/models`，结果经 `setup_tx` 回传（不阻塞事件循环）。
fn spawn_fetch_models(
    state:    &AppState,
    runtime:  &BootstrappedRuntime,
    setup_tx: &mpsc::Sender<Result<Vec<String>, String>>,
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
                Ok(models) => Ok(models.into_iter().map(|mi| mi.id).collect()),
                Err(e)     => Err(format!("{e}")),
            },
            None => Err("could not build provider for model fetch".to_string()),
        };
        let _ = tx.send(result).await;
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
        KeyCode::Char('d') if ctrl => {
            if state.input.is_empty() && !state.is_running {
                return Control::Quit;
            }
        }
        KeyCode::Char('o') if ctrl => {
            state.think_show = !state.think_show;
            let msg = if state.think_show { "thinking: show" } else { "thinking: hide" };
            state.chat.add_system(msg);
        }
        KeyCode::Char('t') if ctrl => {
            state.refresh_tasks();
            state.show_tasks = !state.show_tasks;
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
                // 循环切换权限模式 build → edit → ask
                let next = match runtime.mode {
                    neko_core::ModeName::Build => neko_core::ModeName::Edit,
                    neko_core::ModeName::Edit  => neko_core::ModeName::Ask,
                    neko_core::ModeName::Ask   => neko_core::ModeName::Build,
                };
                runtime.mode = next;
                runtime.permissions.lock().await.set_mode(next);
                state.mode = next.to_string();
            }
        }
        KeyCode::Esc => {
            if state.is_running {
                if let Some(sig) = &state.signal {
                    sig.cancel();
                }
            } else if !state.suggestions.is_empty() {
                // 关闭补全：清空当前 `/` 输入
                state.input.clear();
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
                state.input.set(format!("{} ", val));
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
        KeyCode::Up => {
            if !state.suggestions.is_empty() {
                let n = state.suggestions.len();
                state.suggestion_idx = (state.suggestion_idx + n - 1) % n;
            } else if let Some(prev) = history.prev() {
                state.input.set(prev);
            }
        }
        KeyCode::Down => {
            if !state.suggestions.is_empty() {
                let n = state.suggestions.len();
                state.suggestion_idx = (state.suggestion_idx + 1) % n;
            } else if state.is_running {
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
                    state.status_msg = next.map(|id| format!("sub-agent {}", &id.to_string()[..8]))
                        .or_else(|| Some("main agent".into()));
                }
            } else if let Some(next) = history.next() {
                state.input.set(next);
            }
        }
        KeyCode::PageUp => { state.chat.scroll_up(10); }
        KeyCode::PageDown => { state.chat.scroll_down(10); }
        _ => {}
    }

    // 输入变化后重算命令补全。导航键不改 input → 列表不变、idx 仍有效。
    state.suggestions = crate::repl::commands::command_suggestions(&state.input.value, &runtime.skills);
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
            // 在后台拉取模型列表（可能走网络，如 Anthropic 的 GET /v1/models），
            // 拉完通过 picker_tx 回传，避免阻塞事件循环冻结 UI。
            let Some(provider) = runtime.provider.clone() else {
                state.chat.add_system("No provider configured — run /connect first.");
                return CmdResult::Handled;
            };
            let provider_id = provider.id().to_string();
            let active      = state.model.clone();
            let tx          = picker_tx.clone();
            state.status_msg = Some("loading models…".into());
            tokio::spawn(async move {
                let models = provider.list_models().await.unwrap_or_default();
                let entries = models
                    .into_iter()
                    .map(|m| super::widgets::model_picker::ModelEntry {
                        id:           m.id,
                        display_name: m.display_name,
                    })
                    .collect();
                let picker = ModelPickerModal::new(provider_id, entries, active);
                let _ = tx.send(picker).await;
            });
            CmdResult::Handled
        }
        CommandOutcome::SwitchThinking { enabled, budget, show } => {
            state.think_enabled = enabled;
            if let Some(b) = budget { state.think_budget = b; }
            if let Some(s) = show { state.think_show = s; }
            let show_str = if state.think_show { "show" } else { "hide" };
            if state.think_enabled {
                state.chat.add_system(format!(
                    "thinking ON (budget: {} tokens, display: {})", state.think_budget, show_str
                ));
            } else {
                state.chat.add_system("thinking OFF");
            }
            CmdResult::Handled
        }
        CommandOutcome::Clear => {
            state.chat = ChatWidget::new();
            CmdResult::Handled
        }
        CommandOutcome::Compact => {
            state.chat.add_system("compact is available in plain mode (--no-tui) for now");
            CmdResult::Handled
        }
        CommandOutcome::Resume(id) => {
            resume_session_tui(runtime, ctx, state, id).await;
            CmdResult::Handled
        }
        CommandOutcome::Quit => CmdResult::Quit,
        CommandOutcome::Handled => {
            let trimmed = text.trim();
            if trimmed == "/sessions" || trimmed == "/ls" || trimmed == "/resume" {
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
            } else if let Some(rest) = trimmed.strip_prefix("/memory").or_else(|| trimmed.strip_prefix("/mem")) {
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

    state.chat.add_user(text.clone());

    // 添加用户消息并持久化
    let session_id = runtime.session.meta.id;
    {
        let msg = Message::user_text(text);
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

    let mut executor = crate::agent::orchestrator::build_executor(runtime, provider, Some(perm_tx.clone()));
    executor.thinking_budget = if state.think_enabled { Some(state.think_budget) } else { None };

    let ctx2 = ctx.clone();
    let done2 = done_tx.clone();
    let handle = tokio::spawn(async move {
        let mut guard = ctx2.lock().await;
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
                let new_ctx = crate::agent::AgentContext::from_session(
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

fn draw<B: ratatui::backend::Backend>(term: &mut Terminal<B>, state: &mut AppState) -> Result<()> {
    term.draw(|frame| {
        let area = frame.area();
        let input_lines = state.input.visual_line_count(area.width);
        let layout = AppLayout::compute(area, input_lines);

        // chat area
        let show_welcome = !state.chat.has_conversation();
        if show_welcome {
            frame.render_widget(
                render_welcome(&state.model, &state.mode, &state.cwd),
                layout.chat,
            );
        } else {
            let chat_widget = state.chat.render(layout.chat, state.think_show, state.active_sub_agent);
            frame.render_widget(chat_widget, layout.chat);
        }

        // input box
        let ghost = crate::repl::commands::inline_ghost(&state.input.value, &state.suggestions);
        let arg_hint = crate::repl::commands::argument_hint(&state.input.value);
        frame.render_widget(state.input.render(&state.mode, &ghost, arg_hint, layout.input.width), layout.input);

        // footer（状态栏，输入框下方紧凑行）
        {
            use unicode_width::UnicodeWidthStr;

            let dim = ratatui::style::Style::default().fg(MUTED);
            let bold_dim = dim.add_modifier(ratatui::style::Modifier::BOLD);
            let mcolor = super::theme::mode_color(&state.mode);
            let max_w = layout.footer.width as usize;

            let mut spans: Vec<ratatui::text::Span<'static>> = Vec::new();

            let mode_s = format!("⏵⏵ {}", state.mode);
            spans.push(ratatui::text::Span::styled(mode_s, bold_dim.fg(mcolor)));
            spans.push(ratatui::text::Span::styled(" · ".to_string(), dim));

            spans.push(ratatui::text::Span::styled(state.model.clone(), dim));

            if state.is_running {
                let spin = SPINNER[state.spinner_idx % SPINNER.len()];
                spans.push(ratatui::text::Span::styled(format!(" · {} working", spin), dim));
            }
            if state.skip_perms {
                spans.push(ratatui::text::Span::styled(" · skip-perm".to_string(), dim.fg(ERR)));
            }
            if let Some(ref msg) = state.status_msg {
                spans.push(ratatui::text::Span::styled(format!(" · {}", msg), dim));
            }

            if state.show_token_count && state.tokens > 0 && state.context_window > 0 {
                let pct = state.tokens as f64 / state.context_window as f64 * 100.0;
                let tc = if pct >= 85.0 { ERR }
                    else if pct >= 70.0 { WARN }
                    else { MUTED };
                let tokens_k = state.tokens as f64 / 1000.0;
                let token_str = if tokens_k >= 1000.0 {
                    format!("{:.1}M", tokens_k / 1000.0)
                } else {
                    format!("{:.0}k", tokens_k)
                };
                spans.push(ratatui::text::Span::styled(
                    format!(" · {}({:.0}%)", token_str, pct), dim.fg(tc)));
            }

            let mut right = String::new();
            let (_, in_prog_t, _) = tasks::counts(&state.tasks);
            if in_prog_t > 0 {
                right.push_str(&format!(" ◼{} · ", in_prog_t));
            }
            if !state.queued_messages.is_empty() {
                right.push_str(&format!(" queued({}) · ", state.queued_messages.len()));
            }
            right.push_str("Tab ↻ mode");
            if !sub_agent_ids(&state.chat).is_empty() {
                right.push_str(" · ↓ agent");
            }
            right.push_str(" · ? help");

            let right_w = right.width();
            let used: usize = spans.iter().map(|s| s.content.width()).sum();
            if used + right_w + 2 <= max_w {
                let pad = max_w - used - right_w;
                spans.push(ratatui::text::Span::raw(" ".repeat(pad)));
                spans.push(ratatui::text::Span::styled(right, dim));
            }

            frame.render_widget(
                ratatui::widgets::Paragraph::new(ratatui::text::Line::from(spans)),
                layout.footer,
            );
        }

        // cursor
        if state.pending.is_none() && state.session_picker.is_none()
            && state.model_picker.is_none() && state.provider_setup.is_none()
        {
            let (cx, cy) = state.input.cursor_screen_pos(layout.input);
            frame.set_cursor_position((cx, cy));
        }

        // 浮层
        let show_suggestions = !state.suggestions.is_empty() && state.pending.is_none()
            && state.session_picker.is_none() && state.model_picker.is_none()
            && state.provider_setup.is_none();

        if state.pending.is_none() && !show_suggestions
            && state.show_tasks && !state.tasks.is_empty()
        {
            let panel_area = tasks::area(area, layout.input.y, state.tasks.len());
            frame.render_widget(ratatui::widgets::Clear, panel_area);
            frame.render_widget(tasks::render(&state.tasks), panel_area);
        }

        if show_suggestions {
            let sg_area = suggestions::area(area, layout.input.y, state.suggestions.len());
            frame.render_widget(ratatui::widgets::Clear, sg_area);
            frame.render_widget(
                suggestions::render(&state.suggestions, state.suggestion_idx),
                sg_area,
            );
        }

        // permission select
        if let Some(p) = &state.pending {
            let h = p.modal.height();
            let modal_area = PermissionModal::area(area, layout.input.y, h);
            frame.render_widget(PermissionModal::clear(), modal_area);
            frame.render_widget(p.modal.render(), modal_area);
        }

        // 模型选择器
        if let Some(picker) = &state.model_picker {
            let h = picker.height();
            let picker_area = ModelPickerModal::area(area, layout.input.y, h);
            frame.render_widget(ModelPickerModal::clear(), picker_area);
            frame.render_widget(picker.render(), picker_area);
        }

        // 会话选择器
        if let Some(picker) = &state.session_picker {
            let h = picker.height();
            let picker_area = SessionPickerModal::area(area, layout.input.y, h);
            frame.render_widget(SessionPickerModal::clear(), picker_area);
            frame.render_widget(picker.render(), picker_area);
        }

        // /connect 向导
        if let Some(setup) = &state.provider_setup {
            let h = setup.height();
            let setup_area = ProviderSetupModal::area(area, layout.input.y, h);
            frame.render_widget(ProviderSetupModal::clear(), setup_area);
            frame.render_widget(setup.render(), setup_area);
        }
    }).map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(())
}
