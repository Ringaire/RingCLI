use async_trait::async_trait;
use globset::{Glob, GlobSetBuilder};
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use walkdir::WalkDir;

pub struct GlobTool;

const MAX_RESULTS: usize = 500;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str { "glob" }
    fn description(&self) -> &str { "Find files matching a glob pattern." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern, e.g. src/**/*.rs" },
                "path": { "type": "string", "description": "Root search path (default: session cwd)" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let pattern = match input["pattern"].as_str() {
            Some(p) => p,
            None => return ToolResult::err("missing 'pattern'"),
        };
        let root = input["path"]
            .as_str()
            .map(|p| {
                let pb = std::path::PathBuf::from(p);
                if pb.is_absolute() { pb } else { ctx.cwd.join(pb) }
            })
            .unwrap_or_else(|| ctx.cwd.clone());

        let glob = match Glob::new(pattern) {
            Ok(g) => g,
            Err(e) => return ToolResult::err(format!("invalid glob pattern: {e}")),
        };
        let mut builder = GlobSetBuilder::new();
        builder.add(glob);
        let set = match builder.build() {
            Ok(s) => s,
            Err(e) => return ToolResult::err(format!("glob build error: {e}")),
        };

        let mut matches = Vec::new();
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
        {
            if matches.len() >= MAX_RESULTS { break; }
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            if set.is_match(rel) {
                matches.push(entry.path().to_string_lossy().into_owned());
            }
        }

        if matches.is_empty() {
            return ToolResult::ok_text("(no matches)");
        }
        let mut out = matches.join("\n");
        if matches.len() >= MAX_RESULTS {
            out.push_str(&format!("\n[... truncated at {} results ...]", MAX_RESULTS));
        }
        ToolResult::ok_text(out)
    }
}
