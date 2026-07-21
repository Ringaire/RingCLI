use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::tui::theme::{UI, MUTED, ERR as DELETE_COLOR};
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

pub struct RenameState {
    pub session_id:  uuid::Uuid,
    pub current_title: String,
}

#[derive(Clone, Debug)]
pub enum SessionAction {
    Delete { id: uuid::Uuid, title: String },
    Fork   { id: uuid::Uuid },
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

    pub fn remove_selected(&mut self) {
        let idx = self.list.cursor();
        if idx < self.sessions.len() {
            self.sessions.remove(idx);
            if self.sessions.is_empty() {
                self.list.focus(0);
            } else if idx >= self.sessions.len() {
                self.list.focus(self.sessions.len() - 1);
            }
        }
    }

    pub fn selected_id(&self) -> Option<Uuid> {
        self.sessions.get(self.list.cursor()).map(|s| s.id)
    }

    pub fn selected_title(&self) -> Option<&str> {
        self.sessions.get(self.list.cursor()).map(|s| s.title.as_str())
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

    pub fn render(&self, rename: Option<&RenameState>, action: Option<&SessionAction>) -> Paragraph<'static> {
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

        let mut lines: Vec<Line<'static>> = Vec::new();

        // 重命名输入行
        if let Some(rs) = rename {
            let title = rs.current_title.clone();
            lines.push(Line::from(vec![
                Span::styled("Rename: ", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(title, Style::default().fg(UI)),
                Span::styled("▌", Style::default().fg(UI)),
            ]));
            lines.push(Line::from(Span::styled(
                "Enter confirm  Esc cancel",
                dim,
            )));
        } else if let Some(action) = action {
            // 二次确认提示
            match action {
                SessionAction::Delete { title, .. } => {
                    let label = if title.is_empty() { "(untitled)" } else { title.as_str() };
                    lines.push(Line::from(vec![
                        Span::styled("Delete ", Style::default().fg(DELETE_COLOR).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("\"{label}\""), Style::default().fg(DELETE_COLOR)),
                        Span::styled("?", Style::default().fg(DELETE_COLOR)),
                    ]));
                    lines.push(Line::from(Span::styled(
                        "Press ⌃D again to confirm  Esc cancel",
                        dim,
                    )));
                }
                SessionAction::Fork { .. } => {
                    lines.push(Line::from(vec![
                        Span::styled("Fork ", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                        Span::styled("this session?", Style::default().fg(UI)),
                    ]));
                    lines.push(Line::from(Span::styled(
                        "Press ⌃F again to confirm  Esc cancel",
                        dim,
                    )));
                }
            }
        } else {
            lines.push(Line::from(
                Span::styled("Sessions", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
            ));
            lines.push(Line::from(Span::styled(
                format!("{} sessions  •  ↑↓ navigate  Enter resume  ⌃D del  ⌃R rename  ⌃F fork  Esc cancel", self.sessions.len()),
                dim,
            )));
        }

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
