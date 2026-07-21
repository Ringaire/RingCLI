use async_trait::async_trait;
use ring_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct LspDiagnosticsTool;

#[async_trait]
impl Tool for LspDiagnosticsTool {
    fn name(&self) -> &str { "lsp_diagnostics" }
    fn description(&self) -> &str {
        "Get LSP diagnostics (errors, warnings) for a file or directory by running the appropriate language toolchain."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File or project path to diagnose" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolResult::err("missing 'path'"),
        };
        let path = {
            let pb = std::path::PathBuf::from(path_str);
            if pb.is_absolute() { pb } else { ctx.cwd.join(pb) }
        };

        let lang = detect_language(&path);
        let (cmd, args) = match lang {
            Some("rust") => ("cargo", vec!["check", "--message-format=json"]),
            Some("typescript") | Some("javascript") => ("npx", vec!["tsc", "--noEmit"]),
            Some("python") => ("python", vec!["-m", "pyflakes", path_str]),
            _ => return ToolResult::ok_text("(unsupported language for LSP diagnostics)"),
        };

        let output = tokio::process::Command::new(cmd)
            .args(&args)
            .current_dir(&ctx.cwd)
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = format!("{}{}", stdout, stderr);
                ToolResult::ok_text(if combined.trim().is_empty() { "(no diagnostics)".into() } else { combined })
            }
            Err(e) => ToolResult::err(format!("failed to run {cmd}: {e}")),
        }
    }
}

fn detect_language(path: &std::path::Path) -> Option<&'static str> {
    if path.join("Cargo.toml").exists() { return Some("rust"); }
    if path.join("tsconfig.json").exists() || path.join("package.json").exists() { return Some("typescript"); }
    if path.join("pyproject.toml").exists() || path.join("setup.py").exists() { return Some("python"); }
    path.extension()?.to_str().and_then(|ext| match ext {
        "rs" => Some("rust"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "py" => Some("python"),
        _ => None,
    })
}
