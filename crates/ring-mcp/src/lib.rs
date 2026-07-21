pub mod error;
pub mod protocol;
pub mod transport;
pub mod client;
pub mod bridge;
pub mod prompts;

pub use bridge::McpToolBridge;
pub use client::{McpClient, MCP_PROTOCOL_VERSION};
pub use error::McpError;
pub use prompts::{import_external_prompts, PromptNotFound, SkillPromptProvider};
pub use protocol::{
    McpContent, McpGetPromptResult, McpPrompt, McpPromptArgument, McpPromptMessage,
    McpRequest, McpResource, McpResponse, McpTool,
};
pub use transport::{SseTransport, StdioTransport, Transport};
