//! Linkkit 协议解析器
//!
//! 提供 XML 标签解析、权限闸门、命令执行等核心功能。

mod error;
mod tags;
mod parser;
mod gate;
mod executor;

pub use error::{LinkkitError, LinkkitResult, GateError};
pub use tags::{LinkkitTag, ToolArgs};
pub use parser::LinkkitParser;
pub use gate::LinkkitGate;
pub use executor::LinkkitExecutor;
