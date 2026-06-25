pub mod cmd;
pub mod commands;
pub mod file_complete;
pub mod history;
pub mod printer;

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use neko_core::session;
use neko_core::tools::Message;
use neko_providers::provider::DEFAULT_THINKING_BUDGET;

use neko_engine::{AgentContext, AgentExecutor, TurnResult};
use crate::args::Args;
use crate::bootstrap::{self, BootstrappedRuntime};

use commands::CommandOutcome;
use history::History;

/// REPL 入口：先 bootstrap，再进入 TUI 或 plain 模式。
pub async fn run(session_id: Option<Uuid>, args: &Args) -> Result<()> {
    let runtime = bootstrap::bootstrap(args, session_id).await?;

    if args.no_tui || args.print {
        run_plain(runtime, args).await
    } else {
        crate::tui::run_with_runtime(runtime, args).await
    }
}

/// Plain（无 TUI）模式 REPL。
pub async fn run_plain(mut runtime: BootstrappedRuntime, args: &Args) -> Result<()> {
    print_banner(&runtime);

    let mut ctx = AgentContext::from_session(
        &runtime.session,
        runtime.model.clone(),
        Some(runtime.system_prompt.clone()),
    );

    let mut hist = History::load().await;

    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);

    // 处理初始 prompt（来自命令行参数）
    if let Some(prompt) = &args.prompt {
        if !prompt.trim().is_empty() {
            hist.push(prompt).await;
            process_user_input(&mut runtime, &mut ctx, prompt.clone(), &mut reader).await?;
        }
    }

    loop {
        use tokio::io::AsyncBufReadExt;
        print_prompt(&runtime);

        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            // EOF (Ctrl+D)
            println!();
            break;
        }
        let input = line.trim_end_matches(['\n', '\r']).to_string();
        if input.trim().is_empty() {
            continue;
        }

        match commands::handle(&input, &runtime.skills) {
            CommandOutcome::NotACommand(text) => {
                hist.push(&text).await;
                process_user_input(&mut runtime, &mut ctx, text, &mut reader).await?;
            }
            CommandOutcome::RunSkill { prompt } => {
                process_user_input(&mut runtime, &mut ctx, prompt, &mut reader).await?;
            }
            CommandOutcome::SwitchMode(mode) => {
                runtime.mode = mode;
                runtime.permissions.lock().await.set_mode(mode);
                println!("[mode switched to {}]", mode);
            }
            CommandOutcome::OpenModePicker => {
                println!("usage: /mode ask|edit|plan|build|agent");
            }
            CommandOutcome::SwitchModel(model) => {
                switch_model(&mut runtime, &mut ctx, model).await;
            }
            CommandOutcome::OpenModelPicker => {
                println!("usage: /model <provider/model-id>");
            }
            CommandOutcome::OpenProviderSetup => {
                println!("[/connect] the interactive wizard runs in the TUI.");
                println!("           plain mode: /connect <provider> <apiKey> [baseUrl]");
            }
            CommandOutcome::QuickConnect { provider, api_key, base_url } => {
                quick_connect(&mut runtime, &mut ctx, provider, api_key, base_url).await;
            }
            CommandOutcome::SwitchThinking { enabled, budget } => {
                if enabled {
                    let budget = budget.unwrap_or(DEFAULT_THINKING_BUDGET);
                    println!("[thinking ON (budget: {} tokens)]", budget);
                } else {
                    println!("[thinking OFF]");
                }
            }
            CommandOutcome::ToggleThinkingDisplay => {
                println!("[reasoning display toggled — TUI only]");
            }
            CommandOutcome::SetEffort(level) => {
                if level == "off" {
                    println!("[effort: off (model default)]");
                } else {
                    println!("[effort: {level}]");
                }
            }
            CommandOutcome::NewSession => {
                let new_session = session::create_session(
                    runtime.session.meta.cwd.clone(),
                    Some(runtime.model.clone()),
                ).await;
                ctx = AgentContext::from_session(
                    &new_session,
                    runtime.model.clone(),
                    Some(runtime.system_prompt.clone()),
                );
                runtime.session = new_session;
                println!("[started new conversation]");
            }
            CommandOutcome::Clear => {
                print!("\x1B[2J\x1B[H");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            CommandOutcome::Compact => {
                compact_context(&mut runtime, &mut ctx).await?;
            }
            CommandOutcome::Resume(id) => {
                resume_session(&mut runtime, &mut ctx, id).await?;
            }
            CommandOutcome::OpenSessionPicker => {
                commands::list_sessions().await?;
            }
            CommandOutcome::Quit => break,
            CommandOutcome::EnterPlan(desc) => {
                let prompt = format!(
                    "The user wants to create a plan. Task: {}\n\n\
                     Use `enter_plan_mode` to switch to plan mode, research, \
                     write the plan, then call `exit_plan_mode` to submit.",
                    if desc.is_empty() { "architecture / task planning" } else { &desc }
                );
                process_user_input(&mut runtime, &mut ctx, prompt, &mut reader).await?;
            }
            CommandOutcome::InitAgentsMd => {
                let agents_path = runtime.cwd.join("AGENTS.md");
                if agents_path.exists() {
                    println!("[AGENTS.md already exists at {}. Ask neko to update it, or delete it first.]", agents_path.display());
                } else {
                    let prompt = crate::repl::commands::build_init_prompt(&runtime.cwd);
                    process_user_input(&mut runtime, &mut ctx, prompt, &mut reader).await?;
                }
            }
            CommandOutcome::Handled => {
                let trimmed = input.trim();
                if let Some(rest) = trimmed.strip_prefix("/memory").or_else(|| trimmed.strip_prefix("/mem")) {
                    commands::handle_memory(rest.trim()).await?;
                }
            }
        }
    }

    println!("[session {} saved]", runtime.session.meta.id);
    Ok(())
}

type StdinReader = tokio::io::BufReader<tokio::io::Stdin>;

/// 处理一条用户输入：持久化用户消息，运行 agent turn，流式打印事件。
async fn process_user_input(
    runtime: &mut BootstrappedRuntime,
    ctx:     &mut AgentContext,
    text:    String,
    reader:  &mut StdinReader,
) -> Result<()> {
    // 展开 `@path` 引用：把 cwd 内被引用文件的内容追加到发给模型的消息。
    let cwd = runtime.cwd.clone();
    let attachments = crate::repl::file_complete::expand_mentions(&text, &cwd);
    let model_text = if attachments.is_empty() { text } else { format!("{text}{attachments}") };
    let user_msg = Message::user_text(model_text);
    ctx.add_message(user_msg.clone());
    session::append_message(runtime.session.meta.id, user_msg).await.ok();

    let signal = CancellationToken::new();
    let signal_for_ctrlc = signal.clone();

    // Ctrl+C 在本轮中取消 agent
    let ctrlc_task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal_for_ctrlc.cancel();
        }
    });

    let result = run_agent_turn(runtime, ctx, signal, reader).await;
    ctrlc_task.abort();

    match result {
        TurnResult::Done { .. } => {}
        TurnResult::MaxTurns => {
            println!("\n[reached max turns limit]");
        }
        TurnResult::Cancelled => {
            println!("\n[cancelled]");
        }
        TurnResult::Error(e) => {
            println!("\n[error: {}]", e);
        }
        TurnResult::Continue => {}
    }

    Ok(())
}

/// 构建 executor 并运行一轮（含多 turn 工具循环），同时订阅事件总线打印，
/// 并在收到权限请求时通过 stdin 交互确认。
async fn run_agent_turn(
    runtime: &BootstrappedRuntime,
    ctx:     &mut AgentContext,
    signal:  CancellationToken,
    reader:  &mut StdinReader,
) -> TurnResult {
    let Some(provider) = runtime.provider.clone() else {
        println!("[no provider configured — run /connect <provider> <apiKey> first]");
        return TurnResult::Error("no provider configured".to_string());
    };

    let (perm_tx, mut perm_rx) = tokio::sync::mpsc::channel(8);

    let executor = build_orchestrator_executor(runtime, provider, Some(perm_tx));

    let mut sub = runtime.bus.subscribe();
    let mut printer = printer::PlainPrinter::new();

    let exec_fut = executor.run(ctx, signal);
    tokio::pin!(exec_fut);

    loop {
        tokio::select! {
            biased;
            Some(req) = perm_rx.recv() => {
                printer.finish();
                let decision = prompt_permission_stdin(&req, reader).await;
                let _ = req.responder.send(decision);
            }
            ev = sub.recv() => {
                // lagged/closed 时忽略
                if let Ok(ev) = ev {
                    printer.handle(&ev);
                }
            }
            result = &mut exec_fut => {
                // 排空剩余事件
                while let Ok(ev) = sub.try_recv() {
                    printer.handle(&ev);
                }
                printer.finish();
                return result;
            }
        }
    }
}

/// 在 plain 模式下通过 stdin 询问权限决定。
async fn prompt_permission_stdin(
    req:    &neko_engine::PermissionRequest,
    reader: &mut StdinReader,
) -> neko_engine::PermissionDecision {
    use neko_engine::PermissionDecision;
    use tokio::io::AsyncBufReadExt;
    use std::io::Write;

    println!();
    println!("\x1B[33mPermission required\x1B[0m: {} — {}", req.tool_name, req.input_preview);
    print!("  [y] allow once  [a] allow always  [d] deny  [x] deny always  (default y) > ");
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        // EOF：保守拒绝
        return PermissionDecision::DenyOnce;
    }

    match line.trim().to_lowercase().as_str() {
        "" | "y" | "yes" => PermissionDecision::AllowOnce,
        "a" | "always"   => PermissionDecision::AllowAlways,
        "x"              => PermissionDecision::DenyAlways,
        _                => PermissionDecision::DenyOnce,
    }
}

/// 切换模型（同 provider 内）或 provider/model。
async fn switch_model(runtime: &mut BootstrappedRuntime, ctx: &mut AgentContext, model_ref: String) {
    use crate::connect::SwitchResult;
    match crate::connect::switch_model(runtime, &model_ref).await {
        SwitchResult::Switched { provider, model } => {
            ctx.model = model.clone();
            ctx.system = Some(runtime.system_prompt.clone());
            println!("[switched to {provider}/{model}]");
        }
        SwitchResult::ModelOnly { model } => {
            ctx.model = model.clone();
            ctx.system = Some(runtime.system_prompt.clone());
            println!("[model switched to {model}]");
        }
        SwitchResult::ProviderMissing { provider } => {
            println!("[provider '{provider}' not available]");
        }
        SwitchResult::NoProvider => {
            println!("[no provider configured — run /connect first]");
        }
    }
}

/// `/connect <provider> <key> [url]` 快速配置（plain 模式无交互向导）。
async fn quick_connect(
    runtime:  &mut BootstrappedRuntime,
    ctx:      &mut AgentContext,
    provider: String,
    api_key:  Option<String>,
    base_url: Option<String>,
) {
    use crate::connect::ConnectResult;
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match crate::connect::quick_connect(runtime, &cwd, &provider, api_key, base_url).await {
        ConnectResult::Connected { provider, model } => {
            ctx.model = model.clone();
            ctx.system = Some(runtime.system_prompt.clone());
            println!("[connected — switched to {provider}/{model}]");
        }
        ConnectResult::Rejected(msg) => println!("[{msg}]"),
    }
}

/// 上下文压缩：直接调 provider 生成结构化摘要，替换消息历史。
/// 若已有历史摘要则保留作 pinned prefix，与新摘要拼接形成摘要链。
async fn compact_context(runtime: &mut BootstrappedRuntime, ctx: &mut AgentContext) -> Result<()> {
    use neko_core::tools::ContentBlock;
    use neko_providers::provider::ChatRequest;
    use crate::repl::commands::{
        COMPACT_SYSTEM_PROMPT, render_compact_transcript,
        split_for_compact, build_compact_message,
    };

    let (prior_summary, new_messages) = split_for_compact(&ctx.messages);

    if new_messages.len() < 4 {
        println!("[nothing to compact]");
        return Ok(());
    }

    let Some(provider) = runtime.provider.clone() else {
        println!("[no provider configured — run /connect first]");
        return Ok(());
    };

    println!("[compacting conversation…]");

    let transcript = render_compact_transcript(new_messages);
    let mut req = ChatRequest::new(ctx.model.clone(), vec![Message::user_text(transcript)]);
    req.system      = Some(COMPACT_SYSTEM_PROMPT.to_string());
    req.temperature = Some(0.3);
    req.max_tokens  = 4096;

    let signal = CancellationToken::new();
    let resp = provider.chat(&req, signal).await?;

    let summary_text: String = resp.message.content.iter()
        .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
        .collect::<Vec<_>>().join("\n");
    let summary_text = summary_text.trim().to_string();

    if summary_text.is_empty() {
        println!("[compact produced no summary; keeping full history]");
        return Ok(());
    }

    let n = ctx.messages.len();
    let summary_msgs = build_compact_message(prior_summary.as_deref(), &summary_text);
    ctx.replace_messages(summary_msgs.clone());
    session::replace_messages(runtime.session.meta.id, &summary_msgs).await.ok();

    println!("[compacted: {} messages → summary ({} chars)]", n, summary_text.len());
    Ok(())
}

/// 恢复指定会话。
async fn resume_session(runtime: &mut BootstrappedRuntime, ctx: &mut AgentContext, id: Uuid) -> Result<()> {
    match session::load_session(id).await {
        Some(s) => {
            *ctx = AgentContext::from_session(&s, runtime.model.clone(), Some(runtime.system_prompt.clone()));
            runtime.session = s;
            println!("[resumed session {} with {} messages]", id, ctx.messages.len());
        }
        None => {
            println!("[session {} not found]", id);
        }
    }
    // flush stdout so the message appears before the next prompt
    use std::io::Write;
    let _ = std::io::stdout().flush();
    Ok(())
}

fn print_banner(runtime: &BootstrappedRuntime) {
    println!("neko v{} — terminal AI coding assistant", env!("CARGO_PKG_VERSION"));
    let prov_id = runtime.provider.as_ref().map(|p| p.id().to_string()).unwrap_or_else(|| "(none — /connect)".to_string());
    println!("provider: {}  model: {}  mode: {}", prov_id, runtime.model, runtime.mode);
    if runtime.skip_perms {
        println!("WARNING: --dangerously-skip-permissions active; all tool calls auto-approved");
    }
    println!("type /help for commands, /quit to exit");
    println!();
}

fn print_prompt(runtime: &BootstrappedRuntime) {
    use std::io::Write;
    print!("[{}] you> ", runtime.mode);
    let _ = std::io::stdout().flush();
}

/// 构建主 agent 的 orchestrator executor。
fn build_orchestrator_executor(
    runtime:  &BootstrappedRuntime,
    provider: std::sync::Arc<dyn neko_providers::Provider>,
    perm_tx:  Option<neko_engine::agent::permission::PermissionSender>,
) -> AgentExecutor {
    neko_engine::agent::orchestrator::build_executor(
        runtime.tools.clone(),
        runtime.permissions.clone(),
        runtime.bus.clone(),
        runtime.catalog.clone(),
        runtime.model.clone(),
        runtime.session.meta.id,
        runtime.cwd.clone(),
        runtime.config.session.max_tokens,
        provider,
        perm_tx,
    )
}
