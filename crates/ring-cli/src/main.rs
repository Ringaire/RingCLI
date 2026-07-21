use anyhow::Result;
use tracing::info;

use ring_cli::{args, bootstrap, config, print_mode, rca, repl, sdk, server};

#[tokio::main]
async fn main() -> Result<()> {
    let args = args::parse();
    config::setup_tracing(&args);
    ring_core::session::init_dirs().await?;

    info!(version = env!("CARGO_PKG_VERSION"), "ring starting");

    if args.list_sessions {
        return repl::cmd::list_sessions().await;
    }

    // --serve: start HTTP server
    if let Some(addr) = &args.serve {
        return server::serve(addr).await;
    }

    // --sdk: stdin/stdout NDJSON mode
    if args.sdk {
        return sdk::run(&args).await;
    }

    // --rca: connect to RCA hub as remote worker
    if let Some(hub_url) = &args.rca {
        let mut runtime = bootstrap::bootstrap(&args, None).await?;
        return rca::connect_and_run(hub_url, &mut runtime).await;
    }

    // --continue: resume most recent session in cwd
    if args.r#continue && args.resume.is_none() {
        let sessions = ring_core::session::list_sessions().await;
        if let Some(latest) = sessions.into_iter().max_by_key(|s| s.updated_at) {
            let session_id = Some(latest.id);
            return repl::run(session_id, &args).await;
        }
    }

    if let Some(resume_ref) = &args.resume {
        // --resume with no argument → show session list
        if resume_ref.is_empty() {
            return repl::cmd::list_sessions().await;
        }

        // --resume <uuid> → resume directly
        if let Ok(id) = resume_ref.parse::<uuid::Uuid>() {
            return repl::run(Some(id), &args).await;
        }

        // --resume <title> → fuzzy match by title
        let sessions = ring_core::session::list_sessions().await;
        let lower = resume_ref.to_lowercase();
        let matched = sessions.into_iter().find(|s| {
            s.title.as_deref().map(|t| t.to_lowercase().contains(&lower)).unwrap_or(false)
        });
        if let Some(s) = matched {
            return repl::run(Some(s.id), &args).await;
        }
        eprintln!("No session found matching: {resume_ref}");
        std::process::exit(1);
    }

    // --print with prompt: one-shot mode
    if args.print {
        if let Some(prompt) = &args.prompt {
            return print_mode::run(prompt, &args).await;
        }
        return repl::run(None, &args).await;
    }

    repl::run(None, &args).await
}
