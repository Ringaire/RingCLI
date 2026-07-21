//! 任务面板（待办列表）。
//!
//! 读取 `{session}.todos.json`（由 ring-tools 的 todo 工具写入），渲染为
//! 纯文本任务区：`✔` 完成(绿/删除线) · `◼` 进行中(橙/粗) · `◻` 待办。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::tui::theme::{UI, MUTED, MAIN, OK, INACTIVE, ACCENT};

// ── JSON 镜像（与 ring-tools::tools::todo::TodoItem 对齐）─────────────────────
// 只取渲染用到的字段；其余键（id / priority）由 serde 默认忽略。

#[derive(Debug, Clone, Deserialize)]
pub struct TodoView {
    pub content: String,
    pub status:  TodoStatus,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}



/// 同步读取会话任务列表（文件小，按需读取即可）。
pub fn load_todos(session_id: Uuid) -> Vec<TodoView> {
    let path = ring_core::session::paths::sessions_dir().join(format!("{}.todos.json", session_id));
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// (pending, in_progress, done) 计数。
pub fn counts(todos: &[TodoView]) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for t in todos {
        match t.status {
            TodoStatus::Pending    => c.0 += 1,
            TodoStatus::InProgress => c.1 += 1,
            TodoStatus::Done       => c.2 += 1,
        }
    }
    c
}

/// 任务区高度。
pub fn height(n: usize) -> u16 {
    (n as u16 + 1).clamp(2, 12)
}

/// 锚定在输入框上方的全宽面板区域。
pub fn area(parent: Rect, input_y: u16, n: usize) -> Rect {
    super::core::scroll_list::anchor_above(parent, input_y, height(n))
}

pub fn render(todos: &[TodoView]) -> Paragraph<'static> {
    let (pending, in_progress, done) = counts(todos);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Tasks", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("  {} pending · {} in progress · {} done", pending, in_progress, done),
            Style::default().fg(MUTED),
        ),
    ]));

    for t in todos {
        let (icon, icon_color, text_style) = match t.status {
            TodoStatus::Done => (
                "✔",
                OK,
                Style::default().fg(MUTED).add_modifier(Modifier::CROSSED_OUT),
            ),
            TodoStatus::InProgress => (
                "◼",
                ACCENT,
                Style::default().fg(MAIN).add_modifier(Modifier::BOLD),
            ),
            TodoStatus::Pending => (
                "◻",
                INACTIVE,
                Style::default().fg(INACTIVE),
            ),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(icon_color)),
            Span::styled(t.content.clone(), text_style),
        ]));
    }

    Paragraph::new(lines)
}
