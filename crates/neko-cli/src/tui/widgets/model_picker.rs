use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::tui::theme::{UI, MUTED, OK};

use super::core::scroll_list::{anchor_below, label, pointer, ScrollList};

const MAX_VISIBLE: usize = 12;

pub struct ModelEntry {
    pub id:           String,
    pub display_name: String,
}

pub struct ModelPickerModal {
    pub provider:     String,
    models:           Vec<ModelEntry>,
    active_model:     String,
    list:             ScrollList,
}

impl ModelPickerModal {
    pub fn new(
        provider: impl Into<String>,
        models: Vec<ModelEntry>,
        active_model: impl Into<String>,
    ) -> Self {
        let active = active_model.into();
        let mut list = ScrollList::new(MAX_VISIBLE);
        let cursor = models.iter().position(|m| m.id == active).unwrap_or(0);
        list.focus(cursor);
        Self {
            provider: provider.into(),
            models,
            active_model: active,
            list,
        }
    }

    pub fn move_up(&mut self) {
        self.list.up(self.models.len());
    }

    pub fn move_down(&mut self) {
        self.list.down(self.models.len());
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.models.get(self.list.cursor()).map(|m| m.id.as_str())
    }

    pub fn height(&self) -> u16 {
        if self.models.is_empty() {
            4
        } else {
            // 2 行 header（标题 + 副标题）
            self.list.body_height(self.models.len(), 2)
        }
    }

    pub fn area(parent: Rect, y: u16, h: u16) -> Rect {
        anchor_below(parent, y, h)
    }

    pub fn render(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);

        if self.models.is_empty() {
            return Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("Switch Model", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("  —  {}", self.provider), Style::default().fg(UI)),
                ]),
                Line::from(Span::styled(format!("No models available for {}", self.provider), dim)),
                Line::from(Span::styled("Esc to cancel", dim)),
            ]);
        }

        let mut lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                Span::styled("Switch Model", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  —  {}", self.provider), Style::default().fg(UI)),
            ]),
            Line::from(Span::styled(
                format!("{} models  •  ↑↓ navigate  Enter select  Esc cancel", self.models.len()),
                dim,
            )),
        ];

        let active = self.active_model.clone();
        lines.extend(self.list.render_rows(&self.models, dim, move |m, rs| {
            let mut spans = vec![pointer(rs.selected), label(rs.selected, m.display_name.clone())];
            if m.id == active {
                spans.push(Span::styled(" ◀", Style::default().fg(OK)));
            }
            Line::from(spans)
        }));

        Paragraph::new(lines)
    }

    pub fn clear() -> Clear {
        Clear
    }
}
