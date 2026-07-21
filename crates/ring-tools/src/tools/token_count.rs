use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct TokenCountTool;

#[async_trait]
impl Tool for TokenCountTool {
    fn name(&self) -> &str { "token_count" }
    fn description(&self) -> &str { "Estimate token count of a file or string using the GPT-3 tokenization heuristic (chars/4)." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Text to count (mutually exclusive with path)" },
                "path": { "type": "string", "description": "File path to count" }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let text = if let Some(t) = input["text"].as_str() {
            t.to_string()
        } else if let Some(p) = input["path"].as_str() {
            let path = {
                let pb = std::path::PathBuf::from(p);
                if pb.is_absolute() { pb } else { ctx.cwd.join(pb) }
            };
            match tokio::fs::read_to_string(&path).await {
                Ok(s) => s,
                Err(e) => return ToolResult::err(format!("{}: {}", path.display(), e)),
            }
        } else {
            return ToolResult::err("provide either 'text' or 'path'");
        };

        let chars  = text.chars().count();
        let tokens = chars.div_ceil(4);
        ToolResult::ok_text(format!("chars: {}\nestimated tokens: {}", chars, tokens))
    }
}
