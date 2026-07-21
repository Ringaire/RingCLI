//! 登出选择器：列出已存储凭据的 provider，选择后清除。
//!
//! `/logout` 无参数时触发，锚定在输入框正下方的 footer zone 中。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Clear,
};

use crate::tui::theme::{INACTIVE, MUTED, UI};

/// 凭证条目。
pub struct CredentialEntry {
    pub id:      String,
    pub display: String,
}

pub struct LogoutPickerModal {
    entries: Vec<CredentialEntry>,
    cursor:  usize,
}

impl LogoutPickerModal {
    pub fn new(entries: Vec<(String, String)>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(id, display)| CredentialEntry { id, display })
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

    pub fn selected_id(&self) -> Option<&str> {
        self.entries.get(self.cursor).map(|e| e.id.as_str())
    }

    pub fn height(&self) -> u16 {
        // 标题(1) + 提示(1) + 空行(1) + 条目数 + 空行(1) + 脚注(1)
        let items = self.entries.len().max(1);
        4 + items as u16
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
        let _warn = Style::default().fg(ratatui::style::Color::Yellow);

        let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
            Span::styled(
                "Logout Provider",
                Style::default().fg(UI).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  —  ↑↓ navigate · Enter logout · Esc cancel", dim),
        ])];

        lines.push(Line::from(""));

        if self.entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No stored credentials found.",
                dim,
            )));
            lines.push(Line::from(Span::styled(
                "  (API keys from environment variables are not shown here.)",
                dim,
            )));
        } else {
            for (i, e) in self.entries.iter().enumerate() {
                let selected = i == self.cursor;
                let pointer = if selected { "❯ " } else { "  " };
                let name_style = if selected {
                    Style::default().fg(UI).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(INACTIVE)
                };

                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("🔑 {}", e.display), name_style),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Note: only clears stored credentials; env vars & providers.json unchanged.",
            dim,
        )));

        ratatui::widgets::Paragraph::new(lines)
    }

    pub fn clear() -> Clear {
        Clear
    }
}
