//! Linkkit 协议解析器
//!
//! 提供 XML/JSON 标签解析、权限闸门、命令执行等核心功能。
//!
//! ## 支持的格式
//!
//! ### XML 格式（完整功能）
//! ```xml
//! <doc-read name="bash"/>
//! <tool-use name="bash">ls -la</tool-use>
//! <bash>cargo build</bash>
//! ```
//!
//! ### JSON 格式（简化版本）
//! ```json
//! {"doc": "bash", "line": "1-50"}
//! {"use": "bash", "meta": {"command": "ls -la"}}
//! ```

mod error;
mod tags;
mod parser;
mod json;
mod output;
mod gate;
mod executor;

pub use error::{LinkkitError, LinkkitResult, GateError};
pub use tags::{LinkkitTag, ToolArgs};
pub use parser::LinkkitParser;
pub use json::LinkkitJson;
pub use output::{OutputGenerator, OutputLevel};
pub use gate::LinkkitGate;
pub use executor::LinkkitExecutor;
