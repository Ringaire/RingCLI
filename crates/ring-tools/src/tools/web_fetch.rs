use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }
    fn description(&self) -> &str { "Fetch the content of a URL and return its text." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" },
                "timeout": { "type": "integer", "description": "Timeout seconds (default 30)", "minimum": 1, "maximum": 120 }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let url = match input["url"].as_str() {
            Some(u) => u.to_string(),
            None => return ToolResult::err("missing 'url'"),
        };
        let timeout_secs = input["timeout"].as_u64().unwrap_or(30).min(120);
        let signal = ctx.signal.clone();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("ring/0.1 (AI agent web fetch)")
            .build()
            .unwrap_or_default();

        let result = tokio::select! {
            r = client.get(&url).send() => r,
            _ = signal.cancelled() => return ToolResult::err("cancelled"),
        };

        match result {
            Ok(resp) => {
                let status = resp.status().as_u16();
                match resp.text().await {
                    Ok(text) => {
                        let out = if status >= 400 {
                            format!("[HTTP {}]\n{}", status, text)
                        } else {
                            text
                        };
                        ToolResult::ok_text(truncate(&out, 512 * 1024))
                    }
                    Err(e) => ToolResult::err(e.to_string()),
                }
            }
            Err(e) => ToolResult::err(e.to_string()),
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else {
        format!("{}\n[... truncated ...]", &s[..max])
    }
}
