// 把 RingEvent 流式打印到 stdout（plain 模式）

use std::io::Write;
use ring_core::events::RingEvent;

pub struct PlainPrinter {
    /// 当前是否处于 assistant 文本输出中（控制换行）
    in_text:         bool,
    /// 累积的 assistant 文本（供 compact 等场景取用）
    assistant_text:  String,
    /// 是否打印过 thinking 提示
    thinking_shown:  bool,
}

impl PlainPrinter {
    pub fn new() -> Self {
        Self {
            in_text:        false,
            assistant_text: String::new(),
            thinking_shown: false,
        }
    }

    pub fn handle(&mut self, ev: &RingEvent) {
        // 子 agent 事件多缩进一级
        let ind: &str = if ev.is_sub_agent() { "    " } else { "" };
        match ev {
            RingEvent::AgentThinking { .. } => {
                if !self.thinking_shown {
                    print!("ai> ");
                    let _ = std::io::stdout().flush();
                    self.thinking_shown = true;
                }
            }
            RingEvent::AgentReasoning { delta, .. } => {
                // thinking 内容用暗色显示（ANSI dim）
                print!("\x1B[2m{}\x1B[0m", delta);
                let _ = std::io::stdout().flush();
            }
            RingEvent::AgentText { delta, .. } => {
                if !self.in_text {
                    self.in_text = true;
                }
                self.assistant_text.push_str(delta);
                print!("{}", delta);
                let _ = std::io::stdout().flush();
            }
            RingEvent::AgentToolCall { tool_name, .. } => {
                // 工具调用开始前换行
                if self.in_text {
                    println!();
                    self.in_text = false;
                }
                println!("{}  \x1B[36m→ {}\x1B[0m", ind, tool_name);
            }
            RingEvent::ToolStart { tool_name, input, .. } => {
                let preview = summarize_input(input);
                println!("{}  \x1B[90m[{}] {}\x1B[0m", ind, tool_name, preview);
            }
            RingEvent::ToolEnd { tool_name, ok, duration_ms, .. } => {
                let status = if *ok { "\x1B[32mok\x1B[0m" } else { "\x1B[31merror\x1B[0m" };
                println!("{}  \x1B[90m[{}] {} ({}ms)\x1B[0m", ind, tool_name, status, duration_ms);
            }
            RingEvent::ToolPermission { tool_name, .. } => {
                println!("{}  \x1B[33m[permission requested: {}]\x1B[0m", ind, tool_name);
            }
            RingEvent::BashOutput { stream, data, .. } => {
                use ring_core::events::BashStream;
                match stream {
                    BashStream::Stdout => print!("{}", data),
                    BashStream::Stderr => eprint!("{}", data),
                }
                let _ = std::io::stdout().flush();
            }
            RingEvent::AgentError { error, .. } => {
                if self.in_text { println!(); self.in_text = false; }
                println!("  \x1B[31m[error: {}]\x1B[0m", error);
            }
            RingEvent::AgentDone { .. } => {
                if self.in_text {
                    println!();
                    self.in_text = false;
                }
                self.thinking_shown = false;
            }
            RingEvent::AgentSpawned { role, model, task, .. } => {
                if self.in_text { println!(); self.in_text = false; }
                let role_s = role.as_deref().unwrap_or("balanced");
                let task_preview = truncate(task, 60);
                println!("  \x1B[35m⟳ spawned sub-agent [{}/{}]: {}\x1B[0m", role_s, model, task_preview);
            }
            RingEvent::ContextUpdate { .. }
            | RingEvent::ContextTruncate { .. }
            | RingEvent::ContextSummary { .. }
            | RingEvent::AgentReasoningDone { .. }
            | RingEvent::AgentTextDone { .. }
            | RingEvent::SessionStart { .. }
            | RingEvent::SessionMessage { .. }
            | RingEvent::SessionEnd { .. }
            | RingEvent::ProcessReady { .. }
            | RingEvent::ProcessExit { .. } => {}
        }
    }

    /// 一轮结束时调用，确保行尾换行。
    pub fn finish(&mut self) {
        if self.in_text {
            println!();
            self.in_text = false;
        }
        let _ = std::io::stdout().flush();
    }

    /// 取出累积的 assistant 文本（清空内部缓冲）。
    pub fn take_assistant_text(&mut self) -> String {
        std::mem::take(&mut self.assistant_text)
    }
}

impl Default for PlainPrinter {
    fn default() -> Self { Self::new() }
}

/// 工具输入的简要预览（取关键字段，截断）。
fn summarize_input(input: &serde_json::Value) -> String {
    // 常见字段优先
    for key in &["command", "path", "pattern", "query", "url"] {
        if let Some(v) = input.get(*key).and_then(|v| v.as_str()) {
            return truncate(v, 80);
        }
    }
    let s = input.to_string();
    truncate(&s, 80)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}…", truncated)
    }
}
