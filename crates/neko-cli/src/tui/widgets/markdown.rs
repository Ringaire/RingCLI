//! 终端 Markdown 渲染器。
//!
//! pulldown-cmark 解析 → 自写渲染层。
//! 支持：heading / 段落软换行 / 代码块 / 行内代码 / 列表 / 引用 /
//! emphasis / strong / link / rule / task list。
//! 按终端宽度软换行（空格分词 + CJK 逐字断行）。

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::theme::{ACCENT, MAIN, MUTED, UI};

/// 把 Markdown 文本渲染为宽度感知的样式化行。
pub fn render_markdown(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(text, opts);
    let mut r = Renderer::new(width);

    for event in parser {
        r.on_event(event);
    }

    r.finish()
}

// ── 渲染状态机 ────────────────────────────────────────────────────────────────

struct Renderer {
    width:   usize,
    lines:   Vec<Line<'static>>,
    spans:   Vec<Span<'static>>,
    styles:  Vec<Style>,
    prefix:  String,
    in_code: bool,
    code_buf: Vec<String>,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Self {
            width,
            lines: Vec::new(),
            spans: Vec::new(),
            styles: vec![Style::default()],
            prefix: String::new(),
            in_code: false,
            code_buf: Vec::new(),
        }
    }

    fn style(&self) -> Style {
        self.styles.iter().copied().fold(Style::default(), Style::patch)
    }

    fn push(&mut self, text: &str, style: Style) {
        self.spans.push(Span::styled(text.to_string(), style));
    }

    fn flush(&mut self) {
        if self.spans.is_empty() && self.prefix.is_empty() {
            return;
        }
        let mut all = Vec::new();
        if !self.prefix.is_empty() {
            all.push(Span::styled(self.prefix.clone(), Style::default().fg(MUTED)));
        }
        all.append(&mut self.spans);

        let avail = self.width.saturating_sub(self.prefix.width()).max(1);
        let wrapped = wrap_spans(&all, avail);
        self.lines.extend(wrapped);
        self.spans.clear();
    }

    fn flush_code(&mut self) {
        let style = Style::default().fg(MUTED);
        for line in &self.code_buf {
            let text = if line.is_empty() { " ".into() } else { line.clone() };
            self.lines.push(Line::from(Span::styled(text, style)));
        }
        if !self.code_buf.is_empty() {
            self.lines.push(Line::from(""));
        }
        self.code_buf.clear();
    }

    fn on_event(&mut self, event: Event) {
        if self.in_code {
            return self.on_code_event(event);
        }
        match event {
            Event::Text(t) => self.push(&t, self.style()),
            Event::Code(c) => self.push(&c, self.style().fg(ACCENT)),
            Event::SoftBreak | Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                let rule: String = std::iter::repeat('─').take(self.width.min(60)).collect();
                self.lines.push(Line::from(Span::styled(rule, Style::default().fg(MUTED))));
            }
            Event::TaskListMarker(checked) => {
                self.push(if checked { "[x] " } else { "[ ] " }, self.style().fg(MUTED));
            }
            // 未处理的变体
            _ => {}

            // Start
            Event::Start(Tag::Heading { level, .. }) => {
                self.styles.push(self.style().fg(MAIN).add_modifier(Modifier::BOLD));
                let _ = level; // 前缀暂不加（# 太占宽度）
            }
            Event::Start(Tag::Paragraph) => {}
            Event::Start(Tag::CodeBlock(_)) => {
                self.in_code = true;
                self.code_buf.clear();
            }
            Event::Start(Tag::Emphasis) => {
                self.styles.push(self.style().add_modifier(Modifier::ITALIC));
            }
            Event::Start(Tag::Strong) => {
                self.styles.push(self.style().add_modifier(Modifier::BOLD));
            }
            Event::Start(Tag::Strikethrough) => {
                self.styles.push(self.style().add_modifier(Modifier::CROSSED_OUT));
            }
            Event::Start(Tag::Link { .. }) => {
                self.styles.push(self.style().fg(UI));
            }
            Event::Start(Tag::Image { .. }) => {
                self.push("[image]", self.style().fg(MUTED));
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.flush();
                self.prefix.push_str("│ ");
            }
            Event::Start(Tag::Item) => {
                self.flush();
                self.prefix.push_str("• ");
            }
            Event::Start(_) => {}

            // End
            Event::End(TagEnd::Heading(_)) => {
                self.styles.pop();
                self.flush();
                self.lines.push(Line::from(""));
            }
            Event::End(TagEnd::Paragraph) => {
                self.flush();
                self.lines.push(Line::from(""));
            }
            Event::End(TagEnd::CodeBlock) => {
                self.in_code = false;
                self.flush_code();
            }
            Event::End(TagEnd::Emphasis)
            | Event::End(TagEnd::Strong)
            | Event::End(TagEnd::Strikethrough)
            | Event::End(TagEnd::Link) => {
                self.styles.pop();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush();
                if self.prefix.ends_with("│ ") {
                    self.prefix.truncate(self.prefix.len() - 2);
                }
            }
            Event::End(TagEnd::Item) => {
                self.flush();
                if self.prefix.ends_with("• ") {
                    self.prefix.truncate(self.prefix.len() - 2);
                }
            }
            Event::End(_) => {}
        }
    }

    fn on_code_event(&mut self, event: Event) {
        match event {
            Event::Text(t) => {
                let s = t.into_string();
                if s.contains('\n') {
                    for (i, line) in s.split('\n').enumerate() {
                        if i == 0 {
                            if let Some(last) = self.code_buf.last_mut() {
                                last.push_str(line);
                            } else {
                                self.code_buf.push(line.to_string());
                            }
                        } else {
                            self.code_buf.push(line.to_string());
                        }
                    }
                } else if let Some(last) = self.code_buf.last_mut() {
                    last.push_str(&s);
                } else {
                    self.code_buf.push(s);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                self.in_code = false;
                self.flush_code();
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush();
        while self.lines.last().map(|l| l.spans.is_empty()).unwrap_or(false) {
            self.lines.pop();
        }
        if self.lines.is_empty() {
            self.lines.push(Line::from(""));
        }
        self.lines
    }
}

// ── 宽度感知软换行 ────────────────────────────────────────────────────────────

fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from(spans.to_vec())];
    }

    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut row_w = 0usize;

    for span in spans {
        let style = span.style;
        let text = &span.content;
        let mut buf = String::new();

        for ch in text.chars() {
            let cw = ch.width_cjk().unwrap_or(1);
            let can_break = (ch == ' ' && row_w + buf.width_cjk() > 0)
                || (is_cjk(ch) && !buf.is_empty());

            if can_break && row_w + buf.width_cjk() + cw > width {
                // flush buf
                let bw = buf.width_cjk();
                rows.last_mut().unwrap().push(Span::styled(std::mem::take(&mut buf), style));
                if ch == ' ' {
                    // 空格换行，吃掉
                    rows.push(Vec::new());
                    row_w = 0;
                    continue;
                }
                rows.push(Vec::new());
                row_w = 0;
                let _ = bw;
            }

            buf.push(ch);

            // buf 本身超宽：硬切
            if buf.width_cjk() >= width {
                rows.last_mut().unwrap().push(Span::styled(std::mem::take(&mut buf), style));
                rows.push(Vec::new());
                row_w = 0;
            }
        }

        if !buf.is_empty() {
            row_w += buf.width_cjk();
            rows.last_mut().unwrap().push(Span::styled(buf, style));
        }
    }

    rows.into_iter()
        .filter(|r| !r.is_empty())
        .map(Line::from)
        .collect()
}

fn is_cjk(ch: char) -> bool {
    let cp = ch as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x3040..=0x30FF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        || (0xFF00..=0xFFEF).contains(&cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_paragraph() {
        let lines = render_markdown("hello world", 80);
        assert!(!lines.is_empty());
    }

    #[test]
    fn soft_wrap_long_text() {
        let long = "This is a very long line that should be wrapped because it exceeds the terminal width";
        let lines = render_markdown(long, 20);
        assert!(lines.len() > 1, "should wrap");
    }

    #[test]
    fn cjk_wrap() {
        let long = "这是一段很长的中文文本它应该在终端宽度限制内自动换行";
        let lines = render_markdown(long, 20);
        assert!(lines.len() > 1, "CJK should wrap");
    }

    #[test]
    fn heading() {
        let lines = render_markdown("# Title\n\nparagraph", 80);
        assert!(lines.iter().any(|l| l.spans.iter().any(|s| s.content.contains("Title"))));
    }

    #[test]
    fn code_block() {
        let md = "```rust\nfn main() {}\n```\n";
        let lines = render_markdown(md, 80);
        assert!(lines.iter().any(|l| l.spans.iter().any(|s| s.content.contains("fn main"))));
    }
}
