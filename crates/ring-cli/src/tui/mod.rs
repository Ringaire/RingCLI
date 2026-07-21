mod app;
mod layout;
mod theme;
mod widgets;

use anyhow::Result;

use crate::args::Args;
use crate::bootstrap::BootstrappedRuntime;

/// TUI 入口（接收已装配的运行时）。
pub async fn run_with_runtime(runtime: BootstrappedRuntime, args: &Args) -> Result<()> {
    app::run_with_runtime(runtime, args).await
}
