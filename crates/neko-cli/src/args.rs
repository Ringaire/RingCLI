use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "neko", about = "NekoCLI — terminal AI coding assistant", version)]
pub struct Args {
    /// Initial prompt to send (positional)
    pub prompt: Option<String>,

    /// Permission mode: ask | edit | plan | build | agent
    #[arg(long, default_value = "build")]
    pub mode: String,

    /// Print response and exit (non-interactive)
    #[arg(short = 'p', long)]
    pub print: bool,

    /// Output format for --print mode: text | json | stream-json
    #[arg(long, default_value = "text")]
    pub output_format: String,

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

    /// Enable extended thinking
    #[arg(long)]
    pub extended_thinking: bool,

    /// Enable verbose debug logging
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Enable debug with optional category filter
    #[arg(long)]
    pub debug: Option<String>,

    /// Additional directories to allow tool access to
    #[arg(long = "add-dir")]
    pub add_dir: Vec<PathBuf>,

    /// Disable TUI, use plain output
    #[arg(long = "no-tui")]
    pub no_tui: bool,

    /// Start HTTP+WS server (e.g. "127.0.0.1:8765")
    #[arg(long)]
    pub serve: Option<String>,

    /// Connect to RCA hub as remote worker (e.g. "wss://hub.neko.dev")
    #[arg(long)]
    pub rca: Option<String>,

    /// SDK mode: communicate via stdin/stdout NDJSON
    #[arg(long)]
    pub sdk: bool,
}

pub fn parse() -> Args {
    Args::parse()
}
