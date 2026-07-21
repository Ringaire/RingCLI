//! ring-cli 库入口：所有模块在此声明，binary 目标通过 `use ring_cli::*` 调用。

pub mod args;
pub mod bootstrap;
pub mod config;
pub mod connect;
pub mod config_watch;
pub mod mcp_manager;
pub mod print_mode;
pub mod rca;
pub mod repl;
pub mod sdk;
pub mod server;
pub mod tui;
