use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    fn description(&self) -> &str { "Search the web using DuckDuckGo instant answer API." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "count": { "type": "integer", "description": "Max results (default 10)", "minimum": 1, "maximum": 50 }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let query = match input["query"].as_str() {
            Some(q) => q.to_string(),
            None => return ToolResult::err("missing 'query'"),
        };
        let count = input["count"].as_u64().unwrap_or(10).min(50) as usize;
        let signal = ctx.signal.clone();

        let encoded = urlencoding_simple(&query);
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_redirect=1&no_html=1&skip_disambig=1",
            encoded
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("ring/0.1")
            .build()
            .unwrap_or_default();

        let result = tokio::select! {
            r = client.get(&url).send() => r,
            _ = signal.cancelled() => return ToolResult::err("cancelled"),
        };

        match result {
            Ok(resp) => match resp.json::<Value>().await {
                Ok(data) => {
                    let mut out = String::new();
                    if let Some(abs) = data["Abstract"].as_str() {
                        if !abs.is_empty() {
                            out.push_str(&format!("Abstract: {}\n", abs));
                        }
                    }
                    if let Some(results) = data["RelatedTopics"].as_array() {
                        for (i, r) in results.iter().take(count).enumerate() {
                            if let (Some(text), Some(href)) = (r["Text"].as_str(), r["FirstURL"].as_str()) {
                                out.push_str(&format!("{}. {} ({})\n", i + 1, text, href));
                            }
                        }
                    }
                    if out.is_empty() { out = "(no results)".into(); }
                    ToolResult::ok_text(out)
                }
                Err(e) => ToolResult::err(e.to_string()),
            },
            Err(e) => ToolResult::err(e.to_string()),
        }
    }
}

fn urlencoding_simple(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ' ' => out.push('+'),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            other => {
                for byte in other.to_string().as_bytes() {
                    out.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    out
}
