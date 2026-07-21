//! 模型列表刷新选择器：选择已注册的 provider，拉取其 /v1/models 刷新本地缓存。
//!
//! `/refreshmodel` 无参数时触发。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Clear,
};

use crate::tui::theme::{INACTIVE, MUTED, UI};

/// Provider 条目。
pub struct ProviderEntry {
    pub id:   String,
    pub name: String,
}

pub struct RefreshModelPickerModal {
    entries: Vec<ProviderEntry>,
    cursor:  usize,
}

impl RefreshModelPickerModal {
    pub fn new(entries: Vec<(String, String)>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(id, name)| ProviderEntry { id, name })
                .collect(),
            cursor: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
        }
    }

    pub fn selected(&self) -> Option<(&str, &str)> {
        self.entries
            .get(self.cursor)
            .map(|e| (e.id.as_str(), e.name.as_str()))
    }

    pub fn height(&self) -> u16 {
        2 + self.entries.len().max(1) as u16
    }

    pub fn area(parent: Rect, y: u16, h: u16) -> Rect {
        let max_y = parent.y + parent.height;
        let end = (y + h).min(max_y);
        let actual_h = end.saturating_sub(y);
        Rect {
            x: parent.x,
            y,
            width: parent.width,
            height: actual_h,
        }
    }

    pub fn render(&self) -> ratatui::widgets::Paragraph<'static> {
        let dim = Style::default().fg(MUTED);

        let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
            Span::styled(
                "Refresh Model List",
                Style::default().fg(UI).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  —  ↑↓ navigate · Enter refresh · Esc cancel", dim),
        ])];

        if self.entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No providers registered. Run /connect first.",
                dim,
            )));
        } else {
            for (i, e) in self.entries.iter().enumerate() {
                let selected = i == self.cursor;
                let pointer = if selected { "❯ " } else { "  " };
                let style = if selected {
                    Style::default().fg(UI).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(INACTIVE)
                };
                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{:<16} {}", e.id, e.name), style),
                ]));
            }
        }

        ratatui::widgets::Paragraph::new(lines)
    }

    pub fn clear() -> Clear {
        Clear
    }
}
