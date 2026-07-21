use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct ReadFileTool;

const MAX_FILE_BYTES: usize = 10 * 1024 * 1024; // 10 MiB
const MAX_LINES_DEFAULT: usize = 2000;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "Read a file from the filesystem. Supports line offset/limit." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or relative path to the file" },
                "offset": { "type": "integer", "description": "0-based line offset to start reading", "minimum": 0 },
                "limit": { "type": "integer", "description": "Max lines to read", "minimum": 1 }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolResult::err("missing 'path' field"),
        };
        let path = resolve_path(&ctx.cwd, path_str);

        let metadata = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) => return ToolResult::err(format!("{}: {}", path.display(), e)),
        };
        if metadata.len() > MAX_FILE_BYTES as u64 {
            return ToolResult::err(format!(
                "file too large: {} bytes (max {} bytes)",
                metadata.len(), MAX_FILE_BYTES
            ));
        }

        let raw = match tokio::fs::read_to_string(&path).await {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("{}: {}", path.display(), e)),
        };

        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let limit  = input["limit"].as_u64().unwrap_or(MAX_LINES_DEFAULT as u64) as usize;

        let lines: Vec<&str> = raw.lines().collect();
        let total = lines.len();
        let start = offset.min(total);
        let end   = (start + limit).min(total);
        let slice = &lines[start..end];

        let mut out = String::new();
        for (i, line) in slice.iter().enumerate() {
            out.push_str(&format!("{}\t{}\n", start + i + 1, line));
        }
        if end < total {
            out.push_str(&format!("\n[... {} more lines ...]", total - end));
        }

        ToolResult::ok_text(out)
    }
}

fn resolve_path(cwd: &std::path::Path, p: &str) -> std::path::PathBuf {
    let pb = std::path::PathBuf::from(p);
    if pb.is_absolute() { pb } else { cwd.join(pb) }
}
