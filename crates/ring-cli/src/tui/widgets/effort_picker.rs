//! 推理力度选择器：上下滚动选择 off/minimal/low/medium/high/xhigh/max。
//!
//! `/effort` 无参数时触发，锚定在输入框正下方的 footer zone 中。
//! `Shift+Tab` 直接循环切换（不打开 picker）。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::tui::theme::{INACTIVE, MUTED, OK, UI};

const MAX_VISIBLE: usize = 8;

/// 力度条目。
struct EffortEntry {
    level: &'static str,
    desc: &'static str,
}

/// 全部级别（循环顺序）。
const LEVELS: &[EffortEntry] = &[
    EffortEntry { level: "off",     desc: "no reasoning effort, model default" },
    EffortEntry { level: "minimal", desc: "minimal thinking overhead" },
    EffortEntry { level: "low",     desc: "low reasoning effort" },
    EffortEntry { level: "medium",  desc: "balanced reasoning (default)" },
    EffortEntry { level: "high",    desc: "strong reasoning, more tokens" },
    EffortEntry { level: "xhigh",   desc: "extra-high reasoning, selected models only" },
    EffortEntry { level: "max",     desc: "maximum reasoning, selected models only" },
];

pub struct EffortPickerModal {
    cursor: usize,
    active_level: Option<String>,
}

impl EffortPickerModal {
    pub fn new(active_level: Option<String>) -> Self {
        let cursor = LEVELS
            .iter()
            .position(|e| {
                active_level
                    .as_deref()
                    .is_some_and(|l| l == e.level)
            })
            .unwrap_or(0);
        Self {
            cursor,
            active_level,
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < LEVELS.len() {
            self.cursor += 1;
        }
    }

    /// 返回选中的级别字符串。"off" 表示 None（不发送 effort）。
    pub fn selected(&self) -> &str {
        LEVELS[self.cursor].level
    }

    pub fn height(&self) -> u16 {
        // 标题(1) + 提示(1) + 条目数
        2 + LEVELS.len() as u16
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

    pub fn render(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);

        let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
            Span::styled(
                "Reasoning Effort",
                Style::default().fg(UI).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  —  ↑↓ navigate · Enter select · Esc cancel · Shift+Tab cycle",
                dim,
            ),
        ])];

        for (i, e) in LEVELS.iter().enumerate() {
            let selected = i == self.cursor;
            let is_active = self
                .active_level
                .as_deref()
                .is_some_and(|l| l == e.level)
                || (self.active_level.is_none() && e.level == "off");

            let pointer = if selected { "❯ " } else { "  " };
            let name_style = if selected {
                Style::default().fg(UI).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(INACTIVE)
            };

            let mut spans = vec![
                Span::styled(pointer, Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<8}", e.level), name_style),
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
