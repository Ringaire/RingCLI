use async_trait::async_trait;
use neko_core::tools::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

pub struct ShellTool;

// ── 交互式 Shell 会话 ──────────────────────────────────────────────────────────

struct ShellSession {
    child:      Option<Child>,
    stdin:      Option<tokio::process::ChildStdin>,
    output_buf: Arc<Mutex<String>>,
    created_ms: u128,
}

static SESSIONS: std::sync::LazyLock<Arc<Mutex<HashMap<String, ShellSession>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

fn new_session_id() -> String {
    use std::hash::{Hash, Hasher};
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ts.hash(&mut h);
    format!("sh{:016x}", h.finish())
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn key_sequence(key: &str) -> Option<&'static [u8]> {
    match key {
        "ctrl_c"  => Some(b"\x03"),
        "ctrl_d"  => Some(b"\x04"),
        "ctrl_l"  => Some(b"\x0c"),
        "ctrl_u"  => Some(b"\x15"),
        "ctrl_w"  => Some(b"\x17"),
        "ctrl_a"  => Some(b"\x01"),
        "ctrl_e"  => Some(b"\x05"),
        "ctrl_r"  => Some(b"\x12"),
        "enter"   => Some(b"\n"),
        "tab"     => Some(b"\t"),
        "escape"  => Some(b"\x1b"),
        "up"      => Some(b"\x1b[A"),
        "down"    => Some(b"\x1b[B"),
        "right"   => Some(b"\x1b[C"),
        "left"    => Some(b"\x1b[D"),
        "home"    => Some(b"\x1b[H"),
        "end"     => Some(b"\x1b[F"),
        "backspace" => Some(b"\x7f"),
        "delete"  => Some(b"\x1b[3~"),
        _ => None,
    }
}

// ── Tool impl ──────────────────────────────────────────────────────────────────

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str { "shell" }

    fn description(&self) -> &str {
        "Interactive shell session. Start a persistent shell, send keystrokes/text, read output. \
         Sessions persist across turns until killed.\n\n\
         Actions:\n\
         - create: start a new shell session\n\
         - stdin:  send text to the shell's stdin\n\
         - key:    send a keyboard shortcut (ctrl_c, ctrl_d, tab, enter, up, down, etc.)\n\
         - read:   read accumulated output (non-destructive)\n\
         - kill:   terminate the session\n\n\
         Examples:\n\
         - shell(action=\"create\")\n\
         - shell(action=\"stdin\", session_id=\"...\", input=\"ls -la\")\n\
         - shell(action=\"stdin\", session_id=\"...\", input=\"\\n\")  # press Enter\n\
         - shell(action=\"key\", session_id=\"...\", key=\"ctrl_c\")    # Ctrl+C\n\
         - shell(action=\"read\", session_id=\"...\")"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "stdin", "key", "read", "kill"],
                    "description": "Action to perform"
                },
                "session_id": {
                    "type": "string",
                    "description": "Session ID (returned by create). Required for all actions except create."
                },
                "input": {
                    "type": "string",
                    "description": "Text to send to stdin (for stdin action). Use \\n for newline."
                },
                "key": {
                    "type": "string",
                    "description": "Keyboard shortcut name (for key action). One of: ctrl_c, ctrl_d, tab, enter, escape, up, down, left, right, home, end, backspace, delete, ctrl_l, ctrl_u, ctrl_w, ctrl_a, ctrl_e, ctrl_r"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let action = input["action"].as_str().unwrap_or("");

        match action {
            "create" => {
                let sid = new_session_id();
                let cwd: std::path::PathBuf = input["cwd"]
                    .as_str()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                    });

                let mut child = Command::new("bash")
                    .args(["-i"])  // interactive mode (prompt, history, etc.)
                    .current_dir(&cwd)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn();

                let mut child = match child {
                    Ok(c) => c,
                    Err(e) => return ToolResult::err(format!("spawn shell failed: {e}")),
                };

                let stdin   = child.stdin.take();
                let Some(stdout) = child.stdout.take() else {
                    return ToolResult::err("no stdout");
                };
                let Some(stderr) = child.stderr.take() else {
                    return ToolResult::err("no stderr");
                };

                let buf = Arc::new(Mutex::new(String::new()));
                let buf_clone = Arc::clone(&buf);

                // 后台读取 stdout
                let b = Arc::clone(&buf);
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stdout);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                        b.lock().await.push_str(&line);
                        line.clear();
                    }
                });

                // 后台读取 stderr
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stderr);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                        buf_clone.lock().await.push_str(&line);
                        line.clear();
                    }
                });

                let session = ShellSession {
                    child: Some(child),
                    stdin,
                    output_buf: buf,
                    created_ms: now_ms(),
                };

                SESSIONS.lock().await.insert(sid.clone(), session);

                // 给 shell 一点时间输出提示符
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                let buf = {
                    let sessions = SESSIONS.lock().await;
                    sessions.get(&sid).map(|s| Arc::clone(&s.output_buf))
                };
                let output = match buf {
                    Some(b) => b.lock().await.clone(),
                    None => String::new(),
                };

                ToolResult::ok_text(format!(
                    "Shell session created.\nsession_id: {}\n\n{}",
                    sid,
                    if output.is_empty() { "(awaiting input)" } else { &output }
                ))
            }

            "stdin" => {
                let sid = match input["session_id"].as_str() {
                    Some(s) => s,
                    None => return ToolResult::err("missing 'session_id'"),
                };

                let text = input["input"].as_str().unwrap_or("");
                if text.is_empty() {
                    return ToolResult::err("missing 'input'");
                }

                let buf = {
                    let mut sessions = SESSIONS.lock().await;
                    let session = match sessions.get_mut(sid) {
                        Some(s) => s,
                        None => return ToolResult::err(format!("session not found: {sid}")),
                    };

                    if let Some(stdin) = session.stdin.as_mut() {
                        let formatted = text.replace("\\n", "\n");
                        if let Err(e) = stdin.write_all(formatted.as_bytes()).await {
                            return ToolResult::err(format!("write stdin failed: {e}"));
                        }
                        if let Err(e) = stdin.flush().await {
                            return ToolResult::err(format!("flush stdin failed: {e}"));
                        }
                    }

                    Arc::clone(&session.output_buf)
                };

                tokio::time::sleep(std::time::Duration::from_millis(150)).await;

                let output = buf.lock().await.clone();
                if output.is_empty() {
                    ToolResult::ok_text("(sent, no output yet)")
                } else {
                    ToolResult::ok_text(output)
                }
            }

            "key" => {
                let sid = match input["session_id"].as_str() {
                    Some(s) => s,
                    None => return ToolResult::err("missing 'session_id'"),
                };

                let key = match input["key"].as_str() {
                    Some(k) => k,
                    None => return ToolResult::err("missing 'key'"),
                };

                let bytes = match key_sequence(key) {
                    Some(b) => b,
                    None => return ToolResult::err(format!("unknown key: {key}")),
                };

                let buf = {
                    let mut sessions = SESSIONS.lock().await;
                    let session = match sessions.get_mut(sid) {
                        Some(s) => s,
                        None => return ToolResult::err(format!("session not found: {sid}")),
                    };

                    if let Some(stdin) = session.stdin.as_mut() {
                        if let Err(e) = stdin.write_all(bytes).await {
                            return ToolResult::err(format!("write key failed: {e}"));
                        }
                        if let Err(e) = stdin.flush().await {
                            return ToolResult::err(format!("flush failed: {e}"));
                        }
                    }

                    Arc::clone(&session.output_buf)
                };

                let delay = if key == "ctrl_d" || key == "ctrl_c" { 300 } else { 100 };
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;

                let output = buf.lock().await.clone();
                if output.is_empty() {
                    ToolResult::ok_text(format!("sent key: {key}"))
                } else {
                    ToolResult::ok_text(output)
                }
            }

            "read" => {
                let sid = match input["session_id"].as_str() {
                    Some(s) => s,
                    None => return ToolResult::err("missing 'session_id'"),
                };

                let buf = {
                    let sessions = SESSIONS.lock().await;
                    match sessions.get(sid) {
                        Some(s) => Arc::clone(&s.output_buf),
                        None => return ToolResult::err(format!("session not found: {sid}")),
                    }
                };

                let output = buf.lock().await.clone();
                if output.is_empty() {
                    ToolResult::ok_text("(no output yet)")
                } else {
                    ToolResult::ok_text(output)
                }
            }

            "kill" => {
                let sid = match input["session_id"].as_str() {
                    Some(s) => s,
                    None => return ToolResult::err("missing 'session_id'"),
                };

                let mut sessions = SESSIONS.lock().await;
                let mut session = match sessions.remove(sid) {
                    Some(s) => s,
                    None => return ToolResult::err(format!("session not found: {sid}")),
                };

                // 关闭 stdin 发送 EOF
                drop(session.stdin.take());

                // kill child
                if let Some(mut child) = session.child.take() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }

                ToolResult::ok_text(format!("session {sid} killed"))
            }

            _ => ToolResult::err(format!("unknown action: {action}. Use: create, stdin, key, read, kill")),
        }
    }
}
