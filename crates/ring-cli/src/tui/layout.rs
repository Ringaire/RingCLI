use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[allow(dead_code)]
pub struct AppLayout {
    pub chat:   Rect,
    pub input:  Rect,
    pub footer: Rect,
}

impl AppLayout {
    /// 底部对齐布局：chat 填满可用空间，输入框在底部，状态栏在输入框下方。
    ///
    /// - `input_lines`：输入框视觉行数（不含边框）。
    pub fn compute(area: Rect, input_lines: u16) -> Self {
        let max_h = (area.height / 2).max(3);
        let input_h = (input_lines + 2).clamp(3, max_h);
        let footer_h = 1;

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),          // chat（填满所有剩余空间）
                Constraint::Length(input_h), // input（含上边框）
                Constraint::Length(footer_h), // 状态栏（无边框）
                Constraint::Length(2),       // 底部留白，让输入框往上提 2 行
            ])
            .split(area);

        Self {
            chat:   chunks[0],
            input:  chunks[1],
            footer: chunks[2],
        }
    }
}
