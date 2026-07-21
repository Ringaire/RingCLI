use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use ring_core::tools::{Tool, ToolContext, ToolResult};
use crate::client::McpClient;
use crate::protocol::{McpContent, McpRequest};

pub struct McpToolBridge {
    client:      Arc<Mutex<McpClient>>,
    tool_name:   String,
    description: String,
    schema:      serde_json::Value,
}

impl McpToolBridge {
    pub fn new(
        client:      Arc<Mutex<McpClient>>,
        tool_name:   impl Into<String>,
        description: impl Into<String>,
        schema:      serde_json::Value,
    ) -> Self {
        Self {
            client,
            tool_name:   tool_name.into(),
            description: description.into(),
            schema,
        }
    }
}

#[async_trait]
impl Tool for McpToolBridge {
    fn name(&self) -> &str { &self.tool_name }
    fn description(&self) -> &str { &self.description }
    fn input_schema(&self) -> serde_json::Value { self.schema.clone() }

    async fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let req = McpRequest { tool: self.tool_name.clone(), params: input };
        let client = self.client.lock().await;
        match client.call(&req).await {
            Ok(resp) => {
                let text: String = resp.content.iter().filter_map(|c| match c {
                    McpContent::Text { text } => Some(text.as_str()),
                    _ => None,
                }).collect::<Vec<_>>().join("\n");
                if resp.is_error {
                    ToolResult::err(text)
                } else {
                    ToolResult::ok_text(text)
                }
            }
            Err(e) => ToolResult::err(e.to_string()),
        }
    }
}
