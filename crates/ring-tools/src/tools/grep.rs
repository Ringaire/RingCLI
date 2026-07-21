use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use regex::Regex;
use serde_json::{json, Value};
use walkdir::WalkDir;

pub struct GrepTool;

const MAX_MATCHES: usize = 200;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str { "grep" }
    fn description(&self) -> &str { "Search file contents with a regex pattern." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern":     { "type": "string", "description": "Regex pattern to search" },
                "path":        { "type": "string", "description": "File or directory to search" },
                "include":     { "type": "string", "description": "Glob for file filter (e.g. *.rs)" },
                "case_sensitive": { "type": "boolean", "description": "Case sensitive (default true)" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let pattern_str = match input["pattern"].as_str() {
            Some(p) => p,
            None => return ToolResult::err("missing 'pattern'"),
        };
        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(true);

        let pattern = {
            let p = if case_sensitive {
                pattern_str.to_string()
            } else {
                format!("(?i){}", pattern_str)
            };
            match Regex::new(&p) {
                Ok(r) => r,
                Err(e) => return ToolResult::err(format!("invalid regex: {e}")),
            }
        };

        let search_path = input["path"]
            .as_str()
            .map(|p| {
                let pb = std::path::PathBuf::from(p);
                if pb.is_absolute() { pb } else { ctx.cwd.join(pb) }
            })
            .unwrap_or_else(|| ctx.cwd.clone());

        let include_glob = input["include"].as_str().and_then(|g| {
            globset::Glob::new(g).ok().and_then(|g| {
                let mut b = globset::GlobSetBuilder::new();
                b.add(g);
                b.build().ok()
            })
        });

        let mut results: Vec<String> = Vec::new();

        let walker = WalkDir::new(&search_path)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file());

        'outer: for entry in walker {
            if results.len() >= MAX_MATCHES { break; }
            let path = entry.path();
            if let Some(ref set) = include_glob {
                let rel = path.strip_prefix(&search_path).unwrap_or(path);
                if !set.is_match(rel) { continue; }
            }
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for (lineno, line) in content.lines().enumerate() {
                if results.len() >= MAX_MATCHES { break 'outer; }
                if pattern.is_match(line) {
                    results.push(format!("{}:{}: {}", path.display(), lineno + 1, line));
                }
            }
        }

        if results.is_empty() {
            return ToolResult::ok_text("(no matches)");
        }
        let mut out = results.join("\n");
        if results.len() >= MAX_MATCHES {
            out.push_str(&format!("\n[... truncated at {} matches ...]", MAX_MATCHES));
        }
        ToolResult::ok_text(out)
    }
}
