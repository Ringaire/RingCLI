//! 命令补全建议下拉（输入 `/` 时显示，锚定在输入框上方）。
//!
//! 数据来自 [`crate::repl::commands::command_suggestions`]：
//! 选中项前 `❯`，命令名 + 描述，底部一行操作提示，超出窗口时滑动。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::tui::theme::MUTED;

use super::core::scroll_list::{anchor_above, label, pointer};
use crate::repl::commands::Suggestion;

/// 最多同时显示的建议条数（超出滑动）。
const MAX_VISIBLE: usize = 6;

/// 面板高度：可见条数 + 操作提示，超出窗口时为上下「more」指示预留 2 行。
pub fn height(n: usize) -> u16 {
    if n <= MAX_VISIBLE {
        (n as u16 + 1).max(2) // 行 + 提示
    } else {
        MAX_VISIBLE as u16 + 3 // 行 + 上下指示 + 提示
    }
}

/// 锚定在输入框上方的全宽区域。
pub fn area(parent: Rect, input_y: u16, n: usize) -> Rect {
    anchor_above(parent, input_y, height(n))
}

pub fn render(items: &[Suggestion], selected: usize) -> Paragraph<'static> {
    let n = items.len();
    // 滑动窗口：保持选中项可见。
    let start = if selected >= MAX_VISIBLE {
        (selected + 1 - MAX_VISIBLE).min(n.saturating_sub(MAX_VISIBLE))
    } else {
        0
    };
    let end = (start + MAX_VISIBLE).min(n);

    let mut lines: Vec<Line<'static>> = Vec::new();

    if start > 0 {
        lines.push(Line::from(Span::styled(
            format!("  ↑ {} more", start),
            Style::default().fg(MUTED),
        )));
    }

    for (i, it) in items.iter().enumerate().take(end).skip(start) {
        let selected_row = i == selected;
        lines.push(Line::from(vec![
            pointer(selected_row),
            label(selected_row, format!("{:<16}", it.label)),
            Span::styled(it.description.clone(), Style::default().fg(MUTED)),
        ]));
    }

    if end < n {
        lines.push(Line::from(Span::styled(
            format!("  ↓ {} more", n - end),
            Style::default().fg(MUTED),
        )));
    }

    lines.push(Line::from(Span::styled(
        "  ↑↓ navigate · Tab accept · Esc dismiss",
        Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
    )));

    Paragraph::new(lines)
}
