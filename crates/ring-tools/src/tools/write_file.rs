use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str { "Write or overwrite a file with the given content." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to write" },
                "content": { "type": "string", "description": "Content to write" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolResult::err("missing 'path'"),
        };
        let content = match input["content"].as_str() {
            Some(c) => c,
            None => return ToolResult::err("missing 'content'"),
        };

        let path = resolve_path(&ctx.cwd, path_str);

        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult::err(format!("failed to create directories: {e}"));
            }
        }

        match tokio::fs::write(&path, content.as_bytes()).await {
            Ok(()) => ToolResult::ok_text(format!("wrote {} bytes to {}", content.len(), path.display())),
            Err(e) => ToolResult::err(format!("{}: {}", path.display(), e)),
        }
    }
}

fn resolve_path(cwd: &std::path::Path, p: &str) -> std::path::PathBuf {
    let pb = std::path::PathBuf::from(p);
    if pb.is_absolute() { pb } else { cwd.join(pb) }
}
