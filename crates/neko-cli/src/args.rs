use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "neko", about = "NekoCLI — terminal AI coding assistant", version)]
pub struct Args {
    /// Initial prompt to send (positional)
    pub prompt: Option<String>,

    /// Permission mode: ask | edit | auto | bypass
    #[arg(long, default_value = "auto")]
    pub mode: String,

    /// Print response and exit (non-interactive, useful for pipes)
    #[arg(short = 'p', long)]
    pub print: bool,

    /// Continue the most recent conversation in the current directory
    #[arg(short = 'c', long)]
    pub r#continue: bool,

    /// Resume a conversation by session UUID, or open picker with optional search
    #[arg(short = 'r', long)]
    pub resume: Option<String>,

    /// List saved sessions
    #[arg(long)]
    pub list_sessions: bool,

    /// Model to use (overrides config)
    #[arg(long)]
    pub model: Option<String>,

    /// Provider to use (overrides config)
    #[arg(long)]
    pub provider: Option<String>,

    /// Working directory (default: current dir)
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Skip all permission checks
    #[arg(long = "dangerously-skip-permissions")]
    pub dangerously_skip_permissions: bool,

    /// Enable extended thinking (Anthropic only)
    #[arg(long)]
    pub extended_thinking: bool,

    /// Enable verbose debug logging
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Enable debug with optional category filter (e.g. "api,hooks")
    #[arg(long)]
    pub debug: Option<String>,

    /// Additional directories to allow tool access to
    #[arg(long = "add-dir")]
    pub add_dir: Vec<PathBuf>,

    /// Disable TUI, use plain output (same as --print)
    #[arg(long = "no-tui")]
    pub no_tui: bool,
}

pub fn parse() -> Args {
    Args::parse()
}
