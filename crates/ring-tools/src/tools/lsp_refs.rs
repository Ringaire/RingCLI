use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use regex::Regex;
use serde_json::{json, Value};
use walkdir::WalkDir;

pub struct LspRefsTool;

const MAX_REFS: usize = 100;

#[async_trait]
impl Tool for LspRefsTool {
    fn name(&self) -> &str { "lsp_refs" }
    fn description(&self) -> &str { "Find all references to a symbol by searching with regex." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "symbol": { "type": "string", "description": "Symbol name to search for" },
                "path": { "type": "string", "description": "Search root (default: session cwd)" },
                "include": { "type": "string", "description": "File glob filter" }
            },
            "required": ["symbol"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let symbol = match input["symbol"].as_str() {
            Some(s) => s,
            None => return ToolResult::err("missing 'symbol'"),
        };
        let root = input["path"]
            .as_str()
            .map(|p| {
                let pb = std::path::PathBuf::from(p);
                if pb.is_absolute() { pb } else { ctx.cwd.join(pb) }
            })
            .unwrap_or_else(|| ctx.cwd.clone());

        let pattern = match Regex::new(&format!(r"\b{}\b", regex::escape(symbol))) {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("regex error: {e}")),
        };

        let mut refs = Vec::new();
        for entry in WalkDir::new(&root).follow_links(false).into_iter().flatten() {
            if refs.len() >= MAX_REFS { break; }
            if !entry.file_type().is_file() { continue; }
            let content = match std::fs::read_to_string(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for (i, line) in content.lines().enumerate() {
                if refs.len() >= MAX_REFS { break; }
                if pattern.is_match(line) {
                    refs.push(format!("{}:{}: {}", entry.path().display(), i + 1, line.trim()));
                }
            }
        }

        if refs.is_empty() { return ToolResult::ok_text("(no references found)"); }
        let mut out = refs.join("\n");
        if refs.len() >= MAX_REFS {
            out.push_str(&format!("\n[... truncated at {} refs ...]", MAX_REFS));
        }
        ToolResult::ok_text(out)
    }
}
