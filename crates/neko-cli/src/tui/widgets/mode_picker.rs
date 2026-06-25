//! 权限模式选择器：上下滚动选择 ask/edit/plan/build/agent。
//!
//! `/mode` 无参数时触发，锚定在输入框正下方的 footer zone 中。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use neko_core::permissions::ModeName;

use crate::tui::theme::{INACTIVE, MUTED, OK, UI};

const MAX_VISIBLE: usize = 6;

/// 模式条目。
struct ModeEntry {
    mode: ModeName,
    name: &'static str,
    desc: &'static str,
}

/// 全部模式（Tab 循环顺序）。
fn entries() -> [ModeEntry; 5] {
    [
        ModeEntry { mode: ModeName::Ask,   name: "ask",   desc: "read-only — no writes, no shell" },
        ModeEntry { mode: ModeName::Edit,  name: "edit",  desc: "file edits auto-approved, no shell" },
        ModeEntry { mode: ModeName::Plan,  name: "plan",  desc: "read/explore allowed, writes/bash ask" },
        ModeEntry { mode: ModeName::Build, name: "build", desc: "all tools auto-approved" },
        ModeEntry { mode: ModeName::Agent, name: "agent", desc: "fully autonomous, no permission checks" },
    ]
}

pub struct ModePickerModal {
    cursor: usize,
    active: ModeName,
}

impl ModePickerModal {
    pub fn new(active: ModeName) -> Self {
        let entries = entries();
        let cursor = entries
            .iter()
            .position(|e| e.mode == active)
            .unwrap_or(0);
        Self { cursor, active }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < entries().len() {
            self.cursor += 1;
        }
    }

    pub fn selected(&self) -> ModeName {
        entries()[self.cursor].mode
    }

    pub fn height(&self) -> u16 {
        // 标题(1) + 提示(1) + 条目数
        2 + entries().len() as u16
    }

    pub fn area(parent: Rect, y: u16, h: u16) -> Rect {
        let max_y = parent.y + parent.height;
        let end = (y + h).min(max_y);
        let actual_h = end.saturating_sub(y);
        Rect { x: parent.x, y, width: parent.width, height: actual_h }
    }

    pub fn render(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);
        let entries = entries();

        let mut lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                Span::styled(
                    "Switch Mode",
                    Style::default().fg(UI).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  —  ↑↓ navigate · Enter select · Esc cancel", dim),
            ]),
        ];

        for (i, e) in entries.iter().enumerate() {
            let selected = i == self.cursor;
            let is_active = e.mode == self.active;

            let pointer = if selected { "❯ " } else { "  " };
            let name_style = if selected {
                Style::default().fg(UI).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(INACTIVE)
            };

            let mut spans = vec![
                Span::styled(pointer, Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<6}", e.name), name_style),
                Span::styled(format!("  {}", e.desc), dim),
            ];

            if is_active {
                spans.push(Span::styled(" ◀", Style::default().fg(OK)));
            }

            lines.push(Line::from(spans));
        }

        Paragraph::new(lines)
    }

    pub fn clear() -> Clear {
        Clear
    }
}
