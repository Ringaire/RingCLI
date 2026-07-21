pub struct ToolPreview {
    pub summary: String,
}

const SUMMARY_LEN: usize = 80;

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

pub fn extract_tool_preview(tool_name: &str, input: &serde_json::Value) -> ToolPreview {
    let summary = match tool_name {
        "bash" | "run_command" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or_default();
            truncate(cmd.trim(), SUMMARY_LEN)
        }
        "write_file" | "edit_file" => {
            input.get("path").and_then(|v| v.as_str()).unwrap_or_default().to_string()
        }
        "read_file" | "glob" | "grep" | "tree" => {
            let key = ["path", "pattern", "query"].iter()
                .find_map(|k| input.get(*k).and_then(|v| v.as_str()))
                .unwrap_or_default();
            truncate(key, SUMMARY_LEN)
        }
        "web_fetch" | "web_search" => {
            let key = input.get("url")
                .or_else(|| input.get("query"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            truncate(key, SUMMARY_LEN)
        }
        _ => truncate(&input.to_string(), SUMMARY_LEN),
    };
    ToolPreview { summary }
}
