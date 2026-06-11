use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::tui::theme::{UI, MUTED};
use uuid::Uuid;

use super::core::scroll_list::{anchor_above, label, pointer, ScrollList};

const MAX_VISIBLE: usize = 10;

pub struct SessionEntry {
    pub id:            Uuid,
    pub title:         String,
    pub message_count: usize,
    pub updated_at:    String,
}

pub struct SessionPickerModal {
    sessions: Vec<SessionEntry>,
    list:     ScrollList,
}

impl SessionPickerModal {
    pub fn new(sessions: Vec<SessionEntry>) -> Self {
        let mut list = ScrollList::new(MAX_VISIBLE);
        if !sessions.is_empty() {
            list.focus(0);
        }
        Self { sessions, list }
    }

    pub fn move_up(&mut self) {
        self.list.up(self.sessions.len());
    }

    pub fn move_down(&mut self) {
        self.list.down(self.sessions.len());
    }

    pub fn selected_id(&self) -> Option<Uuid> {
        self.sessions.get(self.list.cursor()).map(|s| s.id)
    }

    pub fn height(&self) -> u16 {
        if self.sessions.is_empty() {
            4
        } else {
            self.list.body_height(self.sessions.len(), 2)
        }
    }

    pub fn area(parent: Rect, y: u16, h: u16) -> Rect {
        anchor_above(parent, y, h)
    }

    pub fn render(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);

        if self.sessions.is_empty() {
            return Paragraph::new(vec![
                Line::from(
                    Span::styled("Sessions", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                ),
                Line::from(Span::styled("No saved sessions", dim)),
                Line::from(Span::styled("Esc to close", dim)),
            ]);
        }

        let mut lines: Vec<Line<'static>> = vec![
            Line::from(
                Span::styled("Sessions", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
            ),
            Line::from(Span::styled(
                format!("{} sessions  •  ↑↓ navigate  Enter resume  Esc cancel", self.sessions.len()),
                dim,
            )),
        ];

        lines.extend(self.list.render_rows(&self.sessions, dim, |s, rs| {
            let title = if s.title.is_empty() { "(untitled)" } else { &s.title };
            let line = format!("{} | {} msgs | {}", title, s.message_count, s.updated_at);
            Line::from(vec![pointer(rs.selected), label(rs.selected, line)])
        }));

        Paragraph::new(lines)
    }

    pub fn clear() -> Clear {
        Clear
    }
}
