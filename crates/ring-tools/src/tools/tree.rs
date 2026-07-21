use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::path::Path;

pub struct TreeTool;

const MAX_DEPTH_DEFAULT: usize = 4;
const MAX_ENTRIES: usize = 2000;

/// 默认忽略的目录（除非 ignore_default_ignores=true）
const DEFAULT_IGNORES: &[&str] = &["node_modules", "target", ".git", ".svn", "dist", "build"];

/// 遍历选项（避免参数过多）
struct WalkOpts {
    max_depth:              usize,
    /// 为 true 时不应用 DEFAULT_IGNORES（即显示 node_modules/target 等）
    show_default_ignored:   bool,
    include_hidden:         bool,
}

#[async_trait]
impl Tool for TreeTool {
    fn name(&self) -> &str { "tree" }
    fn description(&self) -> &str { "Show directory tree structure. Respects .gitignore by default." }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Root path (default: session cwd)" },
                "depth": { "type": "integer", "description": "Max depth (default 4)", "minimum": 1, "maximum": 20 },
                "ignore_gitignore": { "type": "boolean", "description": "Ignore .gitignore rules (default false)" },
                "include_hidden": { "type": "boolean", "description": "Include hidden files (default false)" }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let path = input["path"]
            .as_str()
            .map(|p| {
                let pb = std::path::PathBuf::from(p);
                if pb.is_absolute() { pb } else { ctx.cwd.join(pb) }
            })
            .unwrap_or_else(|| ctx.cwd.clone());

        let max_depth = input["depth"].as_u64().unwrap_or(MAX_DEPTH_DEFAULT as u64) as usize;
        let opts = WalkOpts {
            max_depth,
            show_default_ignored: input["ignore_gitignore"].as_bool().unwrap_or(false),
            include_hidden:       input["include_hidden"].as_bool().unwrap_or(false),
        };

        let mut out   = String::new();
        let mut count = 0usize;

        out.push_str(&format!("{}\n", path.display()));
        walk_dir(&path, "", 0, &opts, &mut out, &mut count);

        if count >= MAX_ENTRIES {
            out.push_str(&format!("\n[... tree truncated at {} entries ...]", MAX_ENTRIES));
        }

        ToolResult::ok_text(out)
    }
}

fn walk_dir(
    dir:    &Path,
    prefix: &str,
    depth:  usize,
    opts:   &WalkOpts,
    out:    &mut String,
    count:  &mut usize,
) {
    if depth >= opts.max_depth || *count >= MAX_ENTRIES { return; }

    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };

    let mut entries: Vec<std::fs::DirEntry> = read
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            if !opts.include_hidden && name_str.starts_with('.') { return false; }
            if !opts.show_default_ignored && DEFAULT_IGNORES.contains(&name_str.as_ref()) {
                return false;
            }
            true
        })
        .collect();

    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        b_is_dir.cmp(&a_is_dir).then(a.file_name().cmp(&b.file_name()))
    });

    let len = entries.len();
    for (i, entry) in entries.into_iter().enumerate() {
        if *count >= MAX_ENTRIES { break; }
        let is_last = i == len - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let name = entry.file_name();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let suffix = if is_dir { "/" } else { "" };
        out.push_str(&format!("{}{}{}{}\n", prefix, connector, name.to_string_lossy(), suffix));
        *count += 1;

        if is_dir {
            let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            walk_dir(&entry.path(), &child_prefix, depth + 1, opts, out, count);
        }
    }
}
