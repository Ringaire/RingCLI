// neko-cli 库入口：仅暴露测试所需的公共接口

pub mod args;
pub mod bootstrap;

// 其他模块保持私有，仅供 main.rs 使用
mod config;
mod connect;
mod config_watch;
mod mcp_manager;
mod print_mode;
mod rca;
mod repl;
mod sdk;
mod server;
mod tui;
