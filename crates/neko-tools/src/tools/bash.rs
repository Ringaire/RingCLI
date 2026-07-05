use async_trait::async_trait;
use neko_core::session::paths;
use neko_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::debug;

pub struct BashTool;

const TIMEOUT_DEFAULT_SECS: u64 = 120;
const MAX_OUTPUT_BYTES:  usize = 1_048_576; // 1 MiB 硬截断
const CACHE_THRESHOLD:   usize = 8_192;     // 8 KB 以上写缓存返回预览
const PREVIEW_LINES:     usize = 80;
const TAIL_LINES:        usize = 20;

// ── 后台任务注册表 ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum TaskStatus {
    Running,
    Done { exit_code: i32 },
    Failed(String),
}

#[derive(Clone, Debug)]
struct TaskState {
    command:     String,
    status:      TaskStatus,
    output_path: std::path::PathBuf,
    started_ms:  u128,
    finished_ms: Option<u128>,
}

static TASK_REGISTRY: std::sync::LazyLock<Arc<Mutex<HashMap<String, TaskState>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

fn new_task_id() -> String {
    let ts = now_ms();
    let h: u32 = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        ts.hash(&mut h);
        h.finish() as u32
    };
    format!("{:x}{:08x}", ts & 0xfff, h)
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ── Tool impl ─────────────────────────────────────────────────────────────────

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }

    fn description(&self) -> &str {
        "Run a shell command. Large output is cached to disk with a preview. \
         Set background=true to run async and get a task_id; \
         pass task_id to check status or read output."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to run. Omit when checking a task by task_id."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default 120, max 600). Ignored for background tasks.",
                    "minimum": 1,
                    "maximum": 600
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (defaults to session cwd)"
                },
                "background": {
                    "type": "boolean",
                    "description": "Run in background, return task_id immediately. Use for long-running commands."
                },
                "task_id": {
                    "type": "string",
                    "description": "Check status of a background task. Returns status + output path."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        // ── 查询后台任务状态 ─────────────────────────────────────────────────
        if let Some(task_id) = input["task_id"].as_str() {
            return task_status(task_id);
        }

        let command = match input["command"].as_str() {
            Some(c) => c.to_string(),
            None => return ToolResult::err("missing 'command' or 'task_id' field"),
        };

        let cwd = input["cwd"]
            .as_str()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // ── 后台模式 ─────────────────────────────────────────────────────────
        if input["background"].as_bool().unwrap_or(false) {
            return spawn_background(command, cwd);
        }

        // ── 前台模式 ─────────────────────────────────────────────────────────
        let timeout_secs = input["timeout"].as_u64().unwrap_or(TIMEOUT_DEFAULT_SECS).min(600);
        debug!(cmd = %command, timeout = timeout_secs, "bash foreground");

        let signal = ctx.signal.clone();
        let result = tokio::select! {
            r = run_command(&command, &cwd, timeout_secs) => r,
            _ = signal.cancelled() => return ToolResult::err("command cancelled"),
        };

        match result {
            Ok((stdout, stderr, code)) => {
                let mut out = format_output(&stdout, &stderr, code);
                if out.is_empty() { out = "(no output)".into(); }
                ToolResult::ok_text(out)
            }
            Err(e) => ToolResult::err(e.to_string()),
        }
    }
}

// ── 前台执行 ──────────────────────────────────────────────────────────────────

async fn run_command(
    command: &str,
    cwd:     &std::path::Path,
    timeout_secs: u64,
) -> Result<(String, String, i32), std::io::Error> {
    let child = tokio::process::Command::new("bash")
        .args(["-c", command])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let result = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        child.wait_with_output(),
    ).await;

    match result {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            Ok((stdout, stderr, out.status.code().unwrap_or(-1)))
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "command timed out")),
    }
}

// ── 后台进程 ──────────────────────────────────────────────────────────────────

fn spawn_background(command: String, cwd: std::path::PathBuf) -> ToolResult {
    let task_id = new_task_id();
    let tasks_dir = paths::cache_dir().join("tasks");
    if std::fs::create_dir_all(&tasks_dir).is_err() {
        return ToolResult::err("cannot create tasks cache dir");
    }
    let output_path = tasks_dir.join(format!("bg-{}.txt", task_id));

    let state = TaskState {
        command: command.clone(),
        status:  TaskStatus::Running,
        output_path: output_path.clone(),
        started_ms:  now_ms(),
        finished_ms: None,
    };
    TASK_REGISTRY.lock().unwrap().insert(task_id.clone(), state);

    let registry = Arc::clone(&TASK_REGISTRY);
    let tid = task_id.clone();
    let op  = output_path.clone();

    tokio::spawn(async move {
        let result = run_command(&command, &cwd, 3600).await;
        let (stdout, stderr, code, status) = match result {
            Ok((out, err, code)) => {
                let s = TaskStatus::Done { exit_code: code };
                (out, err, code, s)
            }
            Err(e) => {
                let s = TaskStatus::Failed(e.to_string());
                (String::new(), String::new(), -1, s)
            }
        };

        // 将输出写到缓存文件
        let mut full = stdout;
        if !stderr.is_empty() {
            if !full.ends_with('\n') { full.push('\n'); }
            full.push_str("[stderr]\n");
            full.push_str(&stderr);
        }
        if code != 0 {
            if !full.ends_with('\n') { full.push('\n'); }
            let _ = writeln!(&mut full, "[exit code: {}]", code);
        }
        let _ = std::fs::write(&op, &full);

        if let Ok(mut reg) = registry.lock() {
            if let Some(s) = reg.get_mut(&tid) {
                s.status      = status;
                s.finished_ms = Some(now_ms());
            }
        }
    });

    ToolResult::ok_text(format!(
        "Background task started.\ntask_id: {}\noutput: {}\n\nCheck status: bash(task_id=\"{}\")\nRead output: bash(command=\"cat {}\")",
        task_id, output_path.display(), task_id, output_path.display()
    ))
}

fn task_status(task_id: &str) -> ToolResult {
    let reg = TASK_REGISTRY.lock().unwrap();
    let Some(state) = reg.get(task_id) else {
        return ToolResult::err(format!("unknown task_id: {}", task_id));
    };

    let elapsed_ms = state.finished_ms.unwrap_or_else(now_ms) - state.started_ms;
    let elapsed_s  = elapsed_ms / 1000;

    let status_str = match &state.status {
        TaskStatus::Running            => "running".to_string(),
        TaskStatus::Done { exit_code } => format!("done (exit {})", exit_code),
        TaskStatus::Failed(e)          => format!("failed: {}", e),
    };

    let out = format!(
        "task_id:  {}\ncommand:  {}\nstatus:   {}\nelapsed:  {}s\noutput:   {}\n\nRead output: bash(command=\"cat {}\")",
        task_id, state.command, status_str, elapsed_s,
        state.output_path.display(),
        state.output_path.display(),
    );
    ToolResult::ok_text(out)
}

// ── 输出格式化 + 缓存 ─────────────────────────────────────────────────────────

fn format_output(stdout: &str, stderr: &str, code: i32) -> String {
    let stdout = hard_truncate(stdout);
    let stderr = hard_truncate(stderr);
    let combined_len = stdout.len() + stderr.len();

    if combined_len > CACHE_THRESHOLD {
        cache_and_preview(&stdout, &stderr, code)
    } else {
        let mut out = String::new();
        if !stdout.is_empty() { out.push_str(&stdout); }
        if !stderr.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("[stderr]\n");
            out.push_str(&stderr);
        }
        if code != 0 {
            if !out.is_empty() { out.push('\n'); }
            let _ = write!(out, "[exit code: {}]", code);
        }
        out
    }
}

fn cache_and_preview(stdout: &str, stderr: &str, code: i32) -> String {
    let mut full = String::with_capacity(stdout.len() + stderr.len() + 32);
    full.push_str(stdout);
    if !stderr.is_empty() {
        if !full.ends_with('\n') { full.push('\n'); }
        full.push_str("[stderr]\n");
        full.push_str(stderr);
    }

    let lines: Vec<&str> = full.lines().collect();
    let line_count = lines.len();
    let byte_count = full.len();
    let cache_path = try_write_cache(&full);

    let mut out = String::new();
    if let Some(ref path) = cache_path {
        let _ = writeln!(out, "[large output: {} lines, {} bytes — saved to {}]",
            line_count, byte_count, path.display());
    } else {
        let _ = writeln!(out, "[large output: {} lines, {} bytes]", line_count, byte_count);
    }
    if code != 0 { let _ = writeln!(out, "[exit code: {}]", code); }

    out.push('\n');
    out.push_str(&lines[..PREVIEW_LINES.min(line_count)].join("\n"));

    if line_count > PREVIEW_LINES + TAIL_LINES {
        let _ = write!(out, "\n\n[... {} lines omitted ...]", line_count - PREVIEW_LINES - TAIL_LINES);
        let tail_start = line_count - TAIL_LINES;
        out.push('\n');
        out.push_str(&lines[tail_start..].join("\n"));
    }

    if let Some(ref path) = cache_path {
        let _ = write!(out, "\n\nFull output:\n  cat {}\n  sed -n 'N,Mp' {}", path.display(), path.display());
    }
    out
}

fn try_write_cache(content: &str) -> Option<std::path::PathBuf> {
    let tasks_dir = paths::cache_dir().join("tasks");
    std::fs::create_dir_all(&tasks_dir).ok()?;

    let ts = now_ms();
    let h: u32 = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        ts.hash(&mut h);
        content.len().hash(&mut h);
        h.finish() as u32
    };
    let path = tasks_dir.join(format!("{}-{:08x}.txt", ts, h));
    std::fs::write(&path, content).ok()?;
    Some(path)
}

fn hard_truncate(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES { return s.to_string(); }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    format!("{}\n[... hard-truncated at 1 MiB ...]", &s[..end])
}
