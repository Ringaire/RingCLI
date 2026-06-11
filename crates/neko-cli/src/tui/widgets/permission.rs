use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::tui::theme::{MUTED, MAIN, WARN};
use unicode_width::UnicodeWidthStr;

use super::core::scroll_list::{anchor_below, pointer, ScrollList};

const MAX_VISIBLE: usize = 4;
/// 分隔符行。
const SEP: &str = "───────────────────────────────────────────────────────────────";

pub struct PermissionModal {
    pub tool_name: String,
    pub preview:   String,
    list:          ScrollList,
}

impl PermissionModal {
    pub fn new(tool_name: impl Into<String>, preview: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            preview:   preview.into(),
            list:      ScrollList::wrapping(MAX_VISIBLE),
        }
    }

    pub fn cursor(&self) -> usize {
        self.list.cursor()
    }

    pub fn move_up(&mut self) {
        self.list.up(MAX_VISIBLE);
    }

    pub fn move_down(&mut self) {
        self.list.down(MAX_VISIBLE);
    }

    /// 计算浮层总高度：标题(1) + 分隔(1) + tool行(1) + 空行(1) + command行(最多3) + 空行(1) + 问题(1) + 4选项(4)
    fn calc_height(&self) -> u16 {
        let command_lines = if self.preview.is_empty() {
            1
        } else {
            let w = 60usize.max(1);
            let lines = self.preview.lines().count().max(1);
            let wrapped: usize = self.preview.lines().map(|l| l.width().div_ceil(w).max(1)).sum();
            (lines.max(wrapped)).min(3)
        };
        2 + 1 + 1 + command_lines as u16 + 1 + 1 + 4
    }

    pub fn height(&self) -> u16 {
        self.calc_height()
    }

    /// 锚定在输入框下方。
    pub fn area(parent: Rect, y: u16, h: u16) -> Rect {
        anchor_below(parent, y, h)
    }

    pub fn render(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);
        let bold_white = Style::default().fg(MAIN).add_modifier(Modifier::BOLD);
        let yellow = Style::default().fg(WARN);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // 标题
        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled("Permission Required", bold_white),
        ]));

        // 分隔线
        lines.push(Line::from(Span::styled(SEP, dim)));

        // Tool 行
        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled("Tool: ", dim),
            Span::styled(self.tool_name.clone(), yellow),
        ]));

        // 空行
        lines.push(Line::from(""));

        // Command 预览：截断到 3 行
        if !self.preview.is_empty() {
            let cmd_lines: Vec<&str> = self.preview.lines().collect();
            let max_cmd = cmd_lines.len().min(3);
            for line in cmd_lines.iter().take(max_cmd) {
                let truncated = if line.width() > 62 {
                    let mut s = String::new();
                    let mut w = 0;
                    for ch in line.chars() {
                        let cw = ch.to_string().width();
                        if w + cw > 59 { break; }
                        s.push(ch);
                        w += cw;
                    }
                    s.push('…');
                    s
                } else {
                    (*line).to_string()
                };
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(truncated, Style::default().fg(MAIN)),
                ]));
            }
            if cmd_lines.len() > 3 {
                lines.push(Line::from(Span::styled("  …", dim)));
            }
        } else {
            lines.push(Line::from(""));
        }

        // 空行
        lines.push(Line::from(""));

        // 问题
        lines.push(Line::from(Span::styled(" Allow this command to run?", bold_white)));

        // 4 选项（复用 ScrollList）
        let opts = [
            "Allow once",
            "Always allow this exact command for future sessions",
            "Reject and type something",
            "No",
        ];

        lines.extend(self.list.render_rows(&opts, dim, |opt, rs| {
            Line::from(vec![
                Span::raw(" "),
                pointer(rs.selected),
                if rs.selected {
                    Span::styled(
                        format!("{}. {}", rs.index + 1, opt),
                        Style::default().fg(MAIN).add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(
                        format!("{}. {}", rs.index + 1, opt),
                        Style::default().fg(MUTED),
                    )
                },
            ])
        }));

        Paragraph::new(lines).wrap(Wrap { trim: false })
    }

    pub fn clear() -> Clear {
        Clear
    }
}
