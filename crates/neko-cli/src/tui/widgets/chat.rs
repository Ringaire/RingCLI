use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use super::markdown::render_markdown;
use crate::tui::theme::{ACCENT, ERR, MAIN, MUTED, THINK as THINK_COLOR, UI};

/// AI / 工具圆点（`⏺` 仅 macOS，Linux 用 `●`）。
const BULLET: &str = "●";
/// 用户消息指针。
const POINTER: &str = "❯";
/// 工具输出连接符 (U+23BF)。
const ELBOW: &str = "⎿";
/// 思考指示符。
const THINK_SYM: &str = "✻";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BubbleKind {
    User,
    Assistant,
    Reasoning,
    Tool,
    System,
    Error,
    /// 子 agent 派生通告
    Spawn,
}

#[derive(Debug, Clone)]
pub struct Bubble {
    pub kind:      BubbleKind,
    pub label:     String,
    pub content:   String,
    /// 所属子 agent（None = 主 agent）；用于缩进与着色
    pub sub_agent: Option<Uuid>,
    /// 工具气泡的 call_id（用于关联实时 bash 输出）
    pub call_id:   Option<String>,
    /// 工具气泡：是否完成、是否成功、耗时
    pub tool_done: bool,
    pub tool_ok:   bool,
    pub tool_ms:   u64,
}

impl Bubble {
    fn new(kind: BubbleKind, label: impl Into<String>, content: impl Into<String>, sub_agent: Option<Uuid>) -> Self {
        Self {
            kind,
            label:     label.into(),
            content:   content.into(),
            sub_agent,
            call_id:   None,
            tool_done: false,
            tool_ok:   false,
            tool_ms:   0,
        }
    }
}

pub struct ChatWidget {
    pub bubbles:      Vec<Bubble>,
    /// 每个 agent（None=主）当前正在追加的 assistant 气泡索引
    active_assistant: HashMap<Option<Uuid>, usize>,
    /// 每个 agent 当前正在追加的 reasoning 气泡索引
    active_reasoning: HashMap<Option<Uuid>, usize>,
    /// call_id → tool 气泡索引
    tool_index:       HashMap<String, usize>,
    /// call_id → 实时 bash 输出 (stdout, stderr)
    bash_output:      HashMap<String, (String, String)>,
    /// 向上滚动的行数（0 = 自动跟随最新消息）
    pub scroll_offset: usize,
}

impl ChatWidget {
    pub fn new() -> Self {
        Self {
            bubbles:          Vec::new(),
            active_assistant: HashMap::new(),
            active_reasoning: HashMap::new(),
            tool_index:       HashMap::new(),
            bash_output:      HashMap::new(),
            scroll_offset:    0,
        }
    }

    /// 向上滚动指定行数（鼠标滚轮向上）。
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    /// 向下滚动指定行数（鼠标滚轮向下）。回到 0 = 自动跟随。
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// 是否有实际对话内容（非纯 system 消息）。决定 welcome banner 是否显示。
    pub fn has_conversation(&self) -> bool {
        self.bubbles.iter().any(|b| b.kind != BubbleKind::System)
    }

    /// 追加一段实时 bash 输出。
    pub fn append_bash_output(&mut self, call_id: &str, is_stderr: bool, data: &str) {
        let entry = self.bash_output.entry(call_id.to_string()).or_default();
        if is_stderr {
            entry.1.push_str(data);
        } else {
            entry.0.push_str(data);
        }
    }

    // ── 添加内容 ──────────────────────────────────────────────────────────────

    pub fn add_user(&mut self, text: impl Into<String>) {
        self.bubbles.push(Bubble::new(BubbleKind::User, "you", text, None));
        self.scroll_offset = 0;
        self.reset_streaming();
    }

    pub fn add_system(&mut self, text: impl Into<String>) {
        self.bubbles.push(Bubble::new(BubbleKind::System, "sys", text, None));
    }

    pub fn add_error(&mut self, sub_agent: Option<Uuid>, text: impl Into<String>) {
        self.bubbles.push(Bubble::new(BubbleKind::Error, "err", text, sub_agent));
        self.active_assistant.remove(&sub_agent);
    }

    /// 子 agent 派生通告。
    pub fn add_spawn(&mut self, sub_agent: Uuid, role: &str, model: &str, task: &str) {
        let content = format!("[{}/{}] {}", role, model, task);
        self.bubbles.push(Bubble::new(BubbleKind::Spawn, "spawn", content, Some(sub_agent)));
    }

    pub fn append_assistant(&mut self, sub_agent: Option<Uuid>, delta: &str) {
        match self.active_assistant.get(&sub_agent).copied() {
            Some(i) => {
                if let Some(b) = self.bubbles.get_mut(i) {
                    b.content.push_str(delta);
                }
            }
            None => {
                self.bubbles.push(Bubble::new(BubbleKind::Assistant, "ai", delta, sub_agent));
                self.active_assistant.insert(sub_agent, self.bubbles.len() - 1);
            }
        }
    }

    pub fn append_reasoning(&mut self, sub_agent: Option<Uuid>, delta: &str) {
        match self.active_reasoning.get(&sub_agent).copied() {
            Some(i) => {
                if let Some(b) = self.bubbles.get_mut(i) {
                    b.content.push_str(delta);
                }
            }
            None => {
                self.bubbles.push(Bubble::new(BubbleKind::Reasoning, "think", delta, sub_agent));
                self.active_reasoning.insert(sub_agent, self.bubbles.len() - 1);
            }
        }
    }

    pub fn add_tool(&mut self, sub_agent: Option<Uuid>, call_id: &str, name: &str, preview: &str) {
        // 工具调用开始新段：结束该 agent 当前 assistant 流
        self.active_assistant.remove(&sub_agent);
        self.active_reasoning.remove(&sub_agent);

        // 同一 call_id 去重：AgentToolCall（input=Null）与随后的 ToolStart
        // （带完整 input）会先后到达，应更新同一气泡而非新建。
        if let Some(&idx) = self.tool_index.get(call_id) {
            if let Some(b) = self.bubbles.get_mut(idx) {
                b.label = name.to_string();
                if !preview.is_empty() {
                    b.content = preview.to_string();
                }
            }
            return;
        }

        let content = if preview.is_empty() { String::new() } else { preview.to_string() };
        let mut bubble = Bubble::new(BubbleKind::Tool, name.to_string(), content, sub_agent);
        bubble.call_id = Some(call_id.to_string());
        self.bubbles.push(bubble);
        let idx = self.bubbles.len() - 1;
        self.tool_index.insert(call_id.to_string(), idx);
    }

    pub fn complete_tool(&mut self, call_id: &str, ok: bool, ms: u64) {
        if let Some(&idx) = self.tool_index.get(call_id) {
            if let Some(b) = self.bubbles.get_mut(idx) {
                b.tool_done = true;
                b.tool_ok   = ok;
                b.tool_ms   = ms;
            }
        }
    }

    /// 一轮结束：固化当前流式气泡。
    pub fn end_turn(&mut self) {
        self.reset_streaming();
    }

    fn reset_streaming(&mut self) {
        self.active_assistant.clear();
        self.active_reasoning.clear();
    }

    // ── 渲染 ──────────────────────────────────────────────────────────────────

    /// 当前内容按 `width` 软换行后的总行数。
    pub fn content_height(&self, width: u16, show_reasoning: bool) -> usize {
        self.build_lines(width.max(1) as usize, show_reasoning, None).len()
    }

    /// 构建全部可视行（不做视口截断）。
    /// `filter_sub`：仅显示指定子 agent 的气泡（None = 全部显示）。
    fn build_lines(&self, width: usize, show_reasoning: bool, filter_sub: Option<Uuid>) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        for b in &self.bubbles {
            // 当 show_reasoning 为 false 时跳过 reasoning 气泡
            if !show_reasoning && b.kind == BubbleKind::Reasoning {
                continue;
            }
            // 视图分离：主 agent 和子 agent 的气泡不在同一界面。
            // - 主视图（None）：主 agent 气泡 + Spawn 通告 + System 消息，跳过子 agent 的内容
            // - 子视图（Some）：仅该子 agent 的气泡 + System 消息
            match filter_sub {
                None => {
                    if b.sub_agent.is_some() && b.kind != BubbleKind::Spawn {
                        continue;
                    }
                }
                Some(filter) => {
                    if b.sub_agent != Some(filter) && b.kind != BubbleKind::System {
                        continue;
                    }
                }
            }
            let (indent, indent_w): (&'static str, usize) =
                if b.sub_agent.is_some() { ("  ┆ ", 4) } else { ("", 0) };

            // 行首：可选子 agent 缩进。
            let lead = |spans: &mut Vec<Span<'static>>| {
                if !indent.is_empty() {
                    spans.push(Span::styled(indent, Style::default().fg(MUTED)));
                }
            };

            match b.kind {
                // ── User: `❯ content` ─────────────────────────────────────────
                BubbleKind::User => {
                    let cw = width.saturating_sub(indent_w + 2);
                    for (i, wline) in wrap_text(&b.content, cw).iter().enumerate() {
                        let mut spans = Vec::new();
                        lead(&mut spans);
                        if i == 0 {
                            spans.push(Span::styled(
                                format!("{} ", POINTER),
                                Style::default().fg(UI).add_modifier(Modifier::BOLD),
                            ));
                        } else {
                            spans.push(Span::raw("  "));
                        }
                        spans.push(Span::styled(wline.clone(), Style::default().fg(MAIN)));
                        lines.push(Line::from(spans));
                    }
                    lines.push(Line::from(""));
                }

                // ── Assistant: `● ` + Markdown ────────────────────────────────
                BubbleKind::Assistant => {
                    let cw = width.saturating_sub(indent_w + 2);
                    let md = render_markdown(&b.content, cw);
                    for (i, mdline) in md.into_iter().enumerate() {
                        let mut spans = Vec::new();
                        lead(&mut spans);
                        if i == 0 {
                            spans.push(Span::styled(
                                format!("{} ", BULLET),
                                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                            ));
                        } else {
                            spans.push(Span::raw("  "));
                        }
                        spans.extend(mdline.spans);
                        lines.push(Line::from(spans));
                    }
                    lines.push(Line::from(""));
                }

                // ── Tool: `● name(arg)` + `  ⎿  status` ───────────────────────
                BubbleKind::Tool => {
                    let mut spans = Vec::new();
                    lead(&mut spans);
                    spans.push(Span::styled(format!("{} ", BULLET), Style::default().fg(ACCENT)));
                    spans.push(Span::styled(
                        b.label.clone(),
                        Style::default().fg(MAIN).add_modifier(Modifier::BOLD),
                    ));
                    if !b.content.is_empty() {
                        let avail = width.saturating_sub(indent_w + b.label.width() + 4);
                        let prev = truncate_display(&b.content, avail);
                        spans.push(Span::styled(format!("({})", prev), Style::default().fg(MUTED)));
                    }
                    lines.push(Line::from(spans));

                    // 结果行
                    let (txt, col) = if b.tool_done {
                        if b.tool_ok {
                            (format!("{}ms", b.tool_ms), MUTED)
                        } else {
                            (format!("error · {}ms", b.tool_ms), ERR)
                        }
                    } else {
                        ("running…".to_string(), MUTED)
                    };
                    let mut s2 = Vec::new();
                    lead(&mut s2);
                    s2.push(Span::styled(format!("  {}  ", ELBOW), Style::default().fg(MUTED)));
                    s2.push(Span::styled(txt, Style::default().fg(col)));
                    lines.push(Line::from(s2));

                    // 运行中：实时 bash 输出末几行
                    if !b.tool_done {
                        if let Some((stdout, stderr)) = b.call_id.as_deref().and_then(|c| self.bash_output.get(c)) {
                            let avail = width.saturating_sub(indent_w + 5);
                            for l in last_lines(stdout, 3) {
                                let mut s = Vec::new();
                                lead(&mut s);
                                s.push(Span::raw("     "));
                                s.push(Span::styled(truncate_display(&l, avail), Style::default().fg(MUTED)));
                                lines.push(Line::from(s));
                            }
                            for l in last_lines(stderr, 3) {
                                let mut s = Vec::new();
                                lead(&mut s);
                                s.push(Span::raw("     "));
                                s.push(Span::styled(truncate_display(&l, avail), Style::default().fg(ERR).add_modifier(Modifier::DIM)));
                                lines.push(Line::from(s));
                            }
                        }
                    }
                }

                // ── System: dim 斜体 ──────────────────────────────────────────
                BubbleKind::System => {
                    let cw = width.saturating_sub(indent_w + 2);
                    for wline in wrap_text(&b.content, cw) {
                        let mut spans = Vec::new();
                        lead(&mut spans);
                        spans.push(Span::styled(
                            wline,
                            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
                        ));
                        lines.push(Line::from(spans));
                    }
                }

                // ── Error: `  ⎿  content` 红 ──────────────────────────────────
                BubbleKind::Error => {
                    let cw = width.saturating_sub(indent_w + 5);
                    for (i, wline) in wrap_text(&b.content, cw).iter().enumerate() {
                        let mut spans = Vec::new();
                        lead(&mut spans);
                        if i == 0 {
                            spans.push(Span::styled(format!("  {}  ", ELBOW), Style::default().fg(ERR)));
                        } else {
                            spans.push(Span::raw("     "));
                        }
                        spans.push(Span::styled(wline.clone(), Style::default().fg(ERR)));
                        lines.push(Line::from(spans));
                    }
                    lines.push(Line::from(""));
                }

                // ── Reasoning: `✻ Thinking… (N chars)` + 多行内容 ─────────────
                BubbleKind::Reasoning => {
                    let nchars = b.content.chars().count();
                    let mut head = Vec::new();
                    lead(&mut head);
                    head.push(Span::styled(
                        format!("{} Thinking… ({} chars)", THINK_SYM, nchars),
                        Style::default().fg(THINK_COLOR).add_modifier(Modifier::DIM),
                    ));
                    lines.push(Line::from(head));

                    let cw = width.saturating_sub(indent_w + 2);
                    for wline in wrap_text(&b.content, cw) {
                        let mut s = Vec::new();
                        lead(&mut s);
                        s.push(Span::raw("  "));
                        s.push(Span::styled(wline, Style::default().fg(MUTED).add_modifier(Modifier::ITALIC)));
                        lines.push(Line::from(s));
                    }
                }

                // ── Spawn: 子 agent 派生通告 ──────────────────────────────────
                BubbleKind::Spawn => {
                    let mut spans = Vec::new();
                    lead(&mut spans);
                    spans.push(Span::styled("⟳ ", Style::default().fg(THINK_COLOR)));
                    spans.push(Span::styled(
                        b.content.clone(),
                        Style::default().fg(THINK_COLOR).add_modifier(Modifier::DIM),
                    ));
                    lines.push(Line::from(spans));
                }
            }
        }

        lines
    }

    /// 渲染为 Paragraph（无边框）。只渲染能塞进 area 的末尾行数，保证最新内容可见。
    /// `show_reasoning` 控制是否展示 thinking/reasoning 气泡。
    /// `filter_sub`：仅显示指定子 agent 的气泡（None = 全部显示）。
    pub fn render(&self, area: Rect, show_reasoning: bool, filter_sub: Option<Uuid>) -> Paragraph<'static> {
        let width = area.width.max(1) as usize;
        let max_lines = area.height as usize;
        let mut lines = self.build_lines(width, show_reasoning, filter_sub);

        // 视口处理：
        // - 内容超过一屏：显示末尾 max_lines 行，scroll_offset>0 时向上偏移浏览历史；
        // - 不足一屏：顶部补空行，使内容**底部对齐**贴近输入框（chat-app 观感），而非飘在顶部。
        if lines.len() > max_lines {
            let max_scroll = lines.len() - max_lines;
            let offset = self.scroll_offset.min(max_scroll);
            let end = lines.len() - offset;
            let start = end - max_lines;
            lines = lines.split_off(start);
            lines.truncate(max_lines);
        } else if lines.len() < max_lines {
            let pad = max_lines - lines.len();
            let mut padded: Vec<Line<'static>> = vec![Line::from(""); pad];
            padded.extend(lines);
            lines = padded;
        }

        Paragraph::new(lines)
    }
}

impl Default for ChatWidget {
    fn default() -> Self { Self::new() }
}

/// 取文本末尾的非空 n 行。
fn last_lines(s: &str, n: usize) -> Vec<String> {
    let all: Vec<String> = s.lines().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect();
    let start = all.len().saturating_sub(n);
    all[start..].to_vec()
}

/// 按宽度软换行（保留空行），按 unicode 宽度计算。
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return text.lines().map(|l| l.to_string()).collect();
    }
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_w = 0usize;
        for word in split_keep_spaces(raw_line) {
            let w = word.width();
            if current_w + w > width && !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_w = 0;
            }
            if w > width {
                // 单词本身超宽：硬切
                for ch in word.chars() {
                    let cw = ch.to_string().width();
                    if current_w + cw > width && !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                        current_w = 0;
                    }
                    current.push(ch);
                    current_w += cw;
                }
            } else {
                current.push_str(&word);
                current_w += w;
            }
        }
        out.push(current);
    }
    out
}

/// 切分为词，保留单词后的空格。
fn split_keep_spaces(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in s.chars() {
        buf.push(ch);
        if ch == ' ' {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

fn truncate_display(s: &str, max: usize) -> String {
    if max == 0 { return String::new(); }
    let single = s.replace('\n', " ");
    if single.width() <= max {
        single
    } else {
        let mut out = String::new();
        let mut w = 0;
        for ch in single.chars() {
            let cw = ch.to_string().width();
            if w + cw > max.saturating_sub(1) { break; }
            out.push(ch);
            w += cw;
        }
        out.push('…');
        out
    }
}
