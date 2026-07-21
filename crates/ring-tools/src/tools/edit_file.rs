use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};

pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str { "edit_file" }
    fn description(&self) -> &str {
        "Perform an exact string replacement in a file. Fails if old_string is not found or is ambiguous (found more than once)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path":       { "type": "string", "description": "File path" },
                "old_string": { "type": "string", "description": "Exact text to replace" },
                "new_string": { "type": "string", "description": "Replacement text" },
                "replace_all": { "type": "boolean", "description": "Replace all occurrences (default false)" }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let path_str  = match input["path"].as_str()       { Some(p) => p, None => return ToolResult::err("missing 'path'") };
        let old_str   = match input["old_string"].as_str() { Some(s) => s, None => return ToolResult::err("missing 'old_string'") };
        let new_str   = match input["new_string"].as_str() { Some(s) => s, None => return ToolResult::err("missing 'new_string'") };
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let path = resolve_path(&ctx.cwd, path_str);

        let original = match tokio::fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(format!("{}: {}", path.display(), e)),
        };

        let count = original.matches(old_str).count();
        if count == 0 {
            return ToolResult::err(format!("old_string not found in {}", path.display()));
        }
        if !replace_all && count > 1 {
            return ToolResult::err(format!(
                "old_string found {} times in {} — use replace_all=true or provide more context to make it unique",
                count, path.display()
            ));
        }

        let new_content = if replace_all {
            original.replace(old_str, new_str)
        } else {
            original.replacen(old_str, new_str, 1)
        };

        if let Err(e) = tokio::fs::write(&path, new_content.as_bytes()).await {
            return ToolResult::err(format!("{}: {}", path.display(), e));
        }

        let diff = TextDiff::from_lines(&original, &new_content);
        let mut diff_out = String::new();
        for change in diff.iter_all_changes() {
            let prefix = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal  => " ",
            };
            diff_out.push_str(&format!("{}{}", prefix, change));
        }

        ToolResult::ok_text(format!("edited {}\n\n{}", path.display(), diff_out))
    }
}

fn resolve_path(cwd: &std::path::Path, p: &str) -> std::path::PathBuf {
    let pb = std::path::PathBuf::from(p);
    if pb.is_absolute() { pb } else { cwd.join(pb) }
}
