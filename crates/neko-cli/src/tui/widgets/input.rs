use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::tui::theme::{mode_color, MUTED};
use unicode_width::UnicodeWidthStr;

/// 输入提示符（pointer），由模式决定颜色。
const PROMPT: &str = "❯ ";
/// 提示符显示宽度。
const PROMPT_W: u16 = 2;

/// 多行文本输入控件，带光标定位与字节级游标管理。
pub struct InputWidget {
    /// 文本内容（可含 '\n'）
    pub value:      String,
    /// 光标的字节偏移
    pub cursor_pos: usize,
    /// 是否禁用（agent 运行中）
    pub disabled:   bool,
}

impl InputWidget {
    pub fn new() -> Self {
        Self { value: String::new(), cursor_pos: 0, disabled: false }
    }

    // ── 编辑操作 ──────────────────────────────────────────────────────────────

    pub fn insert_char(&mut self, c: char) {
        if c.is_control() { return; }
        self.value.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.value.insert_str(self.cursor_pos, s);
        self.cursor_pos += s.len();
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos == 0 { return; }
        let prev = self.value[..self.cursor_pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.value.drain(prev..self.cursor_pos);
        self.cursor_pos = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor_pos >= self.value.len() { return; }
        let next = self.value[self.cursor_pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor_pos + i)
            .unwrap_or(self.value.len());
        self.value.drain(self.cursor_pos..next);
    }

    // ── 光标移动 ──────────────────────────────────────────────────────────────

    pub fn move_left(&mut self) {
        if self.cursor_pos == 0 { return; }
        self.cursor_pos = self.value[..self.cursor_pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }

    pub fn move_right(&mut self) {
        if self.cursor_pos >= self.value.len() { return; }
        self.cursor_pos = self.value[self.cursor_pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor_pos + i)
            .unwrap_or(self.value.len());
    }

    pub fn move_home(&mut self) {
        // 移到当前行首
        let before = &self.value[..self.cursor_pos];
        self.cursor_pos = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    }

    pub fn move_end(&mut self) {
        // 移到当前行尾
        let after = &self.value[self.cursor_pos..];
        self.cursor_pos = match after.find('\n') {
            Some(i) => self.cursor_pos + i,
            None    => self.value.len(),
        };
    }

    /// 多行光标上移：跳到上一行相同列（字符列）。
    pub fn move_up(&mut self) {
        let before = &self.value[..self.cursor_pos];
        let cur_line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        if cur_line_start == 0 {
            return; // 第一行
        }
        let col_chars = self.value[cur_line_start..self.cursor_pos].chars().count();
        let prev_end = cur_line_start - 1; // 跳过 \n
        let prev_start = self.value[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
        // 移到上一行的相同列（或行尾）
        let mut new_pos = prev_start;
        for _ in 0..col_chars {
            if let Some(c) = self.value[new_pos..prev_end].chars().next() {
                new_pos += c.len_utf8();
            } else {
                break;
            }
        }
        self.cursor_pos = new_pos;
    }

    /// 多行光标下移：跳到下一行相同列。
    pub fn move_down(&mut self) {
        let before = &self.value[..self.cursor_pos];
        let cur_line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_chars = self.value[cur_line_start..self.cursor_pos].chars().count();
        let after = &self.value[self.cursor_pos..];
        let nl_rel = match after.find('\n') {
            None => return, // 最后一行
            Some(i) => i,
        };
        let next_start = self.cursor_pos + nl_rel + 1;
        let next_end = self.value[next_start..]
            .find('\n')
            .map(|i| next_start + i)
            .unwrap_or(self.value.len());
        // 移到下一行的相同列（或行尾）
        let mut new_pos = next_start;
        for _ in 0..col_chars {
            if new_pos >= next_end {
                break;
            }
            if let Some(c) = self.value[new_pos..next_end].chars().next() {
                new_pos += c.len_utf8();
            } else {
                break;
            }
        }
        self.cursor_pos = new_pos;
    }

    /// 输入框是否包含换行（多行）。
    pub fn is_multiline(&self) -> bool {
        self.value.contains('\n')
    }

    // ── 状态 ──────────────────────────────────────────────────────────────────

    pub fn set(&mut self, text: String) {
        self.cursor_pos = text.len();
        self.value = text;
    }

    pub fn take(&mut self) -> String {
        let s = std::mem::take(&mut self.value);
        self.cursor_pos = 0;
        s
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor_pos = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }

    // ── 渲染 ──────────────────────────────────────────────────────────────────

    /// 文本可用宽度（总内宽减去 `❯ ` 提示符；无左右边框）。
    fn text_width(inner_width: u16) -> usize {
        (inner_width as usize).saturating_sub(PROMPT_W as usize).max(1)
    }

    /// 视觉行数（考虑按 `inner_width` 软换行后的实际行数）。供 layout 计算高度。
    pub fn visual_line_count(&self, inner_width: u16) -> u16 {
        if self.value.is_empty() {
            return 1;
        }
        let w = Self::text_width(inner_width);
        let n: usize = self.value.split('\n').map(|l| wrap_logical(l, w).len()).sum();
        n.max(1) as u16
    }

    /// 渲染输入框（顶部圆角边框，`❯ ` 提示符按模式着色；模式名见状态栏）。
    /// 长行按 `inner_width` 软换行，避免溢出。
    /// `ghost`：行内幽灵补全后缀（灰字，可空）。`arg_hint`：命令参数提示。
    pub fn render<'a>(&'a self, mode: &'a str, ghost: &'a str, arg_hint: Option<&'a str>, inner_width: u16) -> Paragraph<'a> {
        let accent = if self.disabled { MUTED } else { mode_color(mode) };
        let cont = " ".repeat(PROMPT_W as usize); // 续行缩进与 `❯ ` 对齐
        let w = Self::text_width(inner_width);

        // 展开为视觉行：(是否全局首行, 文本)
        let mut visual: Vec<(bool, String)> = Vec::new();
        if self.value.is_empty() {
            visual.push((true, String::new()));
        } else {
            for (li, logical) in self.value.split('\n').enumerate() {
                for (wi, vline) in wrap_logical(logical, w).into_iter().enumerate() {
                    visual.push((li == 0 && wi == 0, vline));
                }
            }
        }

        let mut lines: Vec<Line<'a>> = visual
            .into_iter()
            .map(|(first, text)| {
                if first {
                    Line::from(vec![
                        Span::styled(PROMPT, Style::default().fg(accent)),
                        Span::raw(text),
                    ])
                } else {
                    Line::from(vec![Span::raw(cont.clone()), Span::raw(text)])
                }
            })
            .collect();

        // 行内幽灵补全 + 参数提示：附加到末行尾部（禁用时不显示）。
        if !self.disabled {
            if let Some(last) = lines.last_mut() {
                if !ghost.is_empty() {
                    last.spans.push(Span::styled(ghost, Style::default().fg(MUTED)));
                }
                if let Some(h) = arg_hint {
                    last.spans.push(Span::styled(format!(" {}", h), Style::default().fg(MUTED)));
                }
            }
        }

        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(accent)),
        )
    }

    /// 计算光标在控件内的屏幕坐标 (x, y)，相对于 input area 原点。
    /// 顶部边框占 1 行；`❯ ` / 续行缩进占 `PROMPT_W` 列；长行软换行后逐视觉行定位。
    pub fn cursor_screen_pos(&self, area: Rect) -> (u16, u16) {
        let w = Self::text_width(area.width);
        let before = &self.value[..self.cursor_pos];
        let logical_idx = before.matches('\n').count();
        let cur_line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_str = &self.value[cur_line_start..self.cursor_pos];

        // 光标所在逻辑行之前的所有逻辑行占用的视觉行数。
        let mut visual_row = 0usize;
        for (i, logical) in self.value.split('\n').enumerate() {
            if i == logical_idx {
                break;
            }
            visual_row += wrap_logical(logical, w).len();
        }

        // 光标在当前逻辑行内的软换行行号与列。
        let (wrow, wcol) = wrap_cursor(col_str, w);
        visual_row += wrow;

        let x = area.x + PROMPT_W + wcol as u16;
        let y = area.y + 1 + visual_row as u16;
        (x, y)
    }
}

/// 按显示宽度软换行单条逻辑行（硬切，超出 `width` 就断行）。空行保留为一行。
fn wrap_logical(line: &str, width: usize) -> Vec<String> {
    if width == 0 || line.is_empty() {
        return vec![line.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut col = 0usize;
    for ch in line.chars() {
        let cw = ch.to_string().width();
        if col + cw > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            col = 0;
        }
        cur.push(ch);
        col += cw;
    }
    out.push(cur);
    out
}

/// 给定逻辑行前缀（行首到光标），返回光标的 (软换行行号, 列)。换行逻辑与 `wrap_logical` 一致。
fn wrap_cursor(prefix: &str, width: usize) -> (usize, usize) {
    if width == 0 {
        return (0, prefix.width());
    }
    let mut row = 0usize;
    let mut col = 0usize;
    for ch in prefix.chars() {
        let cw = ch.to_string().width();
        if col + cw > width && col > 0 {
            row += 1;
            col = 0;
        }
        col += cw;
    }
    (row, col)
}

impl Default for InputWidget {
    fn default() -> Self { Self::new() }
}
