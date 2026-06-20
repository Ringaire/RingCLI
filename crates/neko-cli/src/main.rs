mod args;
mod agent;
mod bootstrap;
mod config;
mod connect;
mod config_watch;
mod mcp_manager;
mod repl;
mod tui;

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let args = args::parse();
    config::setup_tracing(&args);
    neko_core::session::init_dirs().await?;

    info!(version = env!("CARGO_PKG_VERSION"), "neko starting");

    if args.list_sessions {
        return repl::cmd::list_sessions().await;
    }

    // --continue: resume most recent session in cwd
    if args.r#continue && args.resume.is_none() {
        let sessions = neko_core::session::list_sessions().await;
        if let Some(latest) = sessions.into_iter().max_by_key(|s| s.updated_at) {
            return repl::run(Some(latest.id), &args).await;
        }
    }

    if let Some(resume_ref) = &args.resume {
        // Accept UUID or search term
        if let Ok(id) = resume_ref.parse::<uuid::Uuid>() {
            return repl::run(Some(id), &args).await;
        }
        // Search term: find session by title match
        let sessions = neko_core::session::list_sessions().await;
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

    repl::run(None, &args).await
}
