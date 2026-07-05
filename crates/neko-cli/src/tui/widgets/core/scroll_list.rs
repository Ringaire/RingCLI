//! `ScrollList<T>` —— 可滚动可选列表的复用原语。
//!
//! 设计（对照 React）：
//! - **state 留在组件**：`cursor`/`scroll_top` 属于 `ScrollList`，跨帧保留。
//! - **data 当 props**：列表元素 `&[T]` 由调用方每帧传入，`ScrollList` 不拥有 `Vec<T>`。
//!   这样同一套导航状态机可服务 model_picker（`ModelEntry`）、provider_setup（`String`）等不同数据。
//! - **render-prop**：每行的具体 `Span` 由调用方闭包决定（React 的 children / render prop）。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::tui::theme::{UI, INACTIVE};

/// 单行渲染时的瞬时状态（props 的一部分，传给行渲染闭包）。
#[derive(Clone, Copy)]
pub struct RowState {
    /// 是否为光标所在行（应画 `❯` + 高亮）。
    pub selected: bool,
    /// 在完整列表中的索引。
    pub index: usize,
}

/// 非受控的列表导航状态。数据不在此处——由调用方以 `&[T]` 每帧传入。
pub struct ScrollList {
    cursor:      usize,
    scroll_top:  usize,
    max_visible: usize,
    /// `true` = 环形（到顶/底回绕，如权限三选一）；`false` = 夹取。
    wrap:        bool,
}

impl ScrollList {
    pub fn new(max_visible: usize) -> Self {
        Self { cursor: 0, scroll_top: 0, max_visible, wrap: false }
    }

    /// 环形导航变体。
    pub fn wrapping(max_visible: usize) -> Self {
        Self { wrap: true, ..Self::new(max_visible) }
    }

    pub fn cursor(&self) -> usize { self.cursor }

    /// 初始光标定位并居中可见窗口（如把 active 模型置中）。
    pub fn focus(&mut self, idx: usize) {
        self.cursor = idx;
        self.scroll_top = idx.saturating_sub(self.max_visible / 2);
    }

    pub fn up(&mut self, len: usize) {
        if len == 0 { return; }
        self.cursor = if self.wrap {
            (self.cursor + len - 1) % len
        } else {
            self.cursor.saturating_sub(1)
        };
        self.follow(len);
    }

    pub fn down(&mut self, len: usize) {
        if len == 0 { return; }
        self.cursor = if self.wrap {
            (self.cursor + 1) % len
        } else {
            (self.cursor + 1).min(len - 1)
        };
        self.follow(len);
    }

    /// 让 `scroll_top` 跟随 `cursor`，保持其可见。
    fn follow(&mut self, len: usize) {
        if self.cursor < self.scroll_top {
            self.scroll_top = self.cursor;
        } else if self.cursor >= self.scroll_top + self.max_visible {
            self.scroll_top = self.cursor + 1 - self.max_visible;
        }
        let max_top = len.saturating_sub(self.max_visible);
        self.scroll_top = self.scroll_top.min(max_top);
    }

    /// 列表占用的行数（含上/下 “N more” 指示行）。`header_lines` 为标题等固定行数。
    pub fn body_height(&self, len: usize, header_lines: u16) -> u16 {
        let visible = len.min(self.max_visible);
        let mut h = header_lines as usize + visible;
        if self.scroll_top > 0 { h += 1; }
        if self.scroll_top + self.max_visible < len { h += 1; }
        h as u16
    }

    /// 渲染可见行（含 `↑ N more` / `↓ N more`）。`row` 闭包决定每行的 `Span`（render-prop）。
    pub fn render_rows<T>(
        &self,
        items:      &[T],
        more_style: Style,
        row:        impl Fn(&T, RowState) -> Line<'static>,
    ) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        if self.scroll_top > 0 {
            lines.push(Line::from(Span::styled(
                format!("  ↑ {} more", self.scroll_top), more_style,
            )));
        }
        let end = (self.scroll_top + self.max_visible).min(items.len());
        for i in self.scroll_top..end {
            lines.push(row(&items[i], RowState { selected: i == self.cursor, index: i }));
        }
        if end < items.len() {
            lines.push(Line::from(Span::styled(
                format!("  ↓ {} more", items.len() - end), more_style,
            )));
        }
        lines
    }
}

/// 把全宽浮层锚定到 `input_y` 上方（吸收各 modal 重复的 `area()` 几何）。
pub fn anchor_above(parent: Rect, input_y: u16, h: u16) -> Rect {
    Rect { x: parent.x, y: input_y.saturating_sub(h), width: parent.width, height: h }
}

/// 把全宽浮层锚定到 `input_y` 下方（用于权限弹窗等需要放在输入框下面的场景）。
pub fn anchor_below(parent: Rect, y: u16, h: u16) -> Rect {
    let max_y = parent.y + parent.height;
    let end = (y + h).min(max_y);
    let actual_h = end.saturating_sub(y);
    Rect { x: parent.x, y, width: parent.width, height: actual_h }
}

/// `❯ ` 焦点指针（选中时），否则两空格占位。
pub fn pointer(selected: bool) -> Span<'static> {
    let s = if selected { "❯ " } else { "  " };
    Span::styled(s, Style::default().fg(UI).add_modifier(Modifier::BOLD))
}

/// 列表项标签：选中时青色加粗，否则灰色。
pub fn label<'a>(selected: bool, text: impl Into<String>) -> Span<'a> {
    let style = if selected {
        Style::default().fg(UI).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(INACTIVE)
    };
    Span::styled(text.into(), style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_navigation_stays_in_bounds() {
        let mut l = ScrollList::new(3);
        l.up(5);                 // 顶部不回绕
        assert_eq!(l.cursor(), 0);
        for _ in 0..10 { l.down(5); }
        assert_eq!(l.cursor(), 4); // 夹到最后一项
    }

    #[test]
    fn wrap_navigation_cycles() {
        let mut l = ScrollList::wrapping(3);
        l.up(3);
        assert_eq!(l.cursor(), 2); // 0 → 回绕到末尾
        l.down(3);
        assert_eq!(l.cursor(), 0);
    }

    #[test]
    fn body_height_counts_more_rows() {
        let mut l = ScrollList::new(3);
        // 10 项、窗口 3：底部有 "↓ more"，无顶部 → header(1)+3+1 = 5
        assert_eq!(l.body_height(10, 1), 5);
        for _ in 0..4 { l.down(10); } // 滚动后顶部也出现 "↑ more"
        assert_eq!(l.body_height(10, 1), 6);
    }

    #[test]
    fn empty_list_no_panic() {
        let mut l = ScrollList::new(3);
        l.up(0);
        l.down(0);
        assert_eq!(l.cursor(), 0);
    }
}
