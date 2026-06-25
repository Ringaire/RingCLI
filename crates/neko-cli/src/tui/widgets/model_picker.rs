//! 模型选择器：跨 provider 分组展示 + 模糊搜索 + role 标签。
//!
//! 对照 opencode `dialog-model.tsx`：
//! - 按 provider 分组展示，当前 provider 置顶
//! - 输入即时过滤，搜索时扁平化（仍保留分组 header 便于辨识）
//! - 模型 ID 转义显示（`-` → 空格，首字母大写）
//! - role 标签：heavy / light / code（balanced 不显示）
//! - 当前模型用 `◀` 标记
//! - 导航：↑↓ / PageUp Down / Home End / Enter / Esc

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use neko_core::agent::{classify_model, ModelRole};

use crate::tui::theme::{INACTIVE, MUTED, OK, UI, WARN};

/// 同时可见的最大行数（含分组 header）。
const MAX_VISIBLE: usize = 14;

pub struct ModelEntry {
    pub id:            String,
    pub display_name:  String,
    pub provider_id:   String,
    pub provider_name: String,
}

/// 渲染行：分组标题或模型项。
enum PickerRow {
    Header { provider_name: String, count: usize },
    Item { model_idx: usize },
}

pub struct ModelPickerModal {
    all_models: Vec<ModelEntry>,
    active_ref: String,
    filter:     String,
    cursor:     usize,
    scroll_top: usize,
    rows:       Vec<PickerRow>,
}

/// 模型 ID 转义为显示名：`-` → 空格，每个词首字母大写。
///
/// `claude-sonnet-4-6` → `Claude Sonnet 4 6`
/// `gpt-4o` → `Gpt 4o`
/// `deepseek-chat` → `Deepseek Chat`
fn prettify_model_id(id: &str) -> String {
    id.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// role 标签文本（balanced 不显示标签）。
fn role_tag(role: ModelRole) -> Option<&'static str> {
    match role {
        ModelRole::Heavy    => Some("heavy"),
        ModelRole::Light    => Some("light"),
        ModelRole::Coding   => Some("code"),
        ModelRole::Balanced => None,
    }
}

/// 取展示名：有 display_name 用 display_name，否则用 prettify_model_id。
fn display_name(m: &ModelEntry) -> String {
    if m.display_name.is_empty() {
        prettify_model_id(&m.id)
    } else {
        m.display_name.clone()
    }
}

impl ModelPickerModal {
    pub fn new(
        models: Vec<ModelEntry>,
        active_provider: impl Into<String>,
        active_model: impl Into<String>,
    ) -> Self {
        let active_ref = format!("{}/{}", active_provider.into(), active_model.into());
        let mut picker = Self {
            all_models: models,
            active_ref,
            filter: String::new(),
            cursor: 0,
            scroll_top: 0,
            rows: Vec::new(),
        };
        picker.rebuild_rows();
        picker.focus_active();
        picker
    }

    /// 重建渲染行：过滤 → 按 provider 分组 → 当前 provider 置顶。
    fn rebuild_rows(&mut self) {
        let f = self.filter.to_lowercase();
        let filtered: Vec<usize> = if f.is_empty() {
            (0..self.all_models.len()).collect()
        } else {
            self.all_models
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m.id.to_lowercase().contains(&f)
                        || m.display_name.to_lowercase().contains(&f)
                        || m.provider_id.to_lowercase().contains(&f)
                        || m.provider_name.to_lowercase().contains(&f)
                })
                .map(|(i, _)| i)
                .collect()
        };

        // 按 provider 分组（保持首次出现顺序）
        let mut groups: Vec<(String, String, Vec<usize>)> = Vec::new();
        for &idx in &filtered {
            let m = &self.all_models[idx];
            if let Some(g) = groups.iter_mut().find(|(p, _, _)| p == &m.provider_id) {
                g.2.push(idx);
            } else {
                groups.push((m.provider_id.clone(), m.provider_name.clone(), vec![idx]));
            }
        }

        // 当前 provider 置顶
        let active_pid = self
            .active_ref
            .split_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        groups.sort_by_key(|(pid, _, _)| pid != &active_pid);

        // 构建 rows
        self.rows.clear();
        for (_, pname, indices) in &groups {
            self.rows.push(PickerRow::Header {
                provider_name: pname.clone(),
                count: indices.len(),
            });
            for &idx in indices {
                self.rows.push(PickerRow::Item { model_idx: idx });
            }
        }

        self.clamp_cursor();
    }

    /// 把 cursor 定位到当前选中模型并居中。
    fn focus_active(&mut self) {
        for (i, row) in self.rows.iter().enumerate() {
            if let PickerRow::Item { model_idx } = row {
                let m = &self.all_models[*model_idx];
                if format!("{}/{}", m.provider_id, m.id) == self.active_ref {
                    self.cursor = i;
                    self.scroll_top = i.saturating_sub(MAX_VISIBLE / 2);
                    return;
                }
            }
        }
        // 没找到 → 定位到第一个 Item
        self.cursor = 0;
        self.scroll_top = 0;
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        if self.rows.is_empty() {
            self.cursor = 0;
            return;
        }
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len() - 1;
        }
        // 确保 cursor 在 Item 行上
        let mut forward = self.cursor;
        while forward < self.rows.len()
            && !matches!(self.rows[forward], PickerRow::Item { .. })
        {
            forward += 1;
        }
        if forward < self.rows.len() {
            self.cursor = forward;
            return;
        }
        // 向前找
        while self.cursor > 0 && !matches!(self.rows[self.cursor], PickerRow::Item { .. }) {
            self.cursor -= 1;
        }
    }

    fn follow_scroll(&mut self) {
        if self.cursor < self.scroll_top {
            self.scroll_top = self.cursor;
        } else if self.cursor >= self.scroll_top + MAX_VISIBLE {
            self.scroll_top = self.cursor + 1 - MAX_VISIBLE;
        }
    }

    // ── 导航 ──────────────────────────────────────────────────────────────────

    pub fn move_up(&mut self) {
        let mut i = self.cursor;
        while i > 0 {
            i -= 1;
            if matches!(self.rows.get(i), Some(PickerRow::Item { .. })) {
                self.cursor = i;
                self.follow_scroll();
                return;
            }
        }
    }

    pub fn move_down(&mut self) {
        let mut i = self.cursor + 1;
        while i < self.rows.len() {
            if matches!(self.rows.get(i), Some(PickerRow::Item { .. })) {
                self.cursor = i;
                self.follow_scroll();
                return;
            }
            i += 1;
        }
    }

    pub fn page_up(&mut self) {
        for _ in 0..10 {
            self.move_up();
        }
    }

    pub fn page_down(&mut self) {
        for _ in 0..10 {
            self.move_down();
        }
    }

    pub fn move_home(&mut self) {
        for i in 0..self.rows.len() {
            if matches!(self.rows.get(i), Some(PickerRow::Item { .. })) {
                self.cursor = i;
                self.scroll_top = 0;
                return;
            }
        }
    }

    pub fn move_end(&mut self) {
        for i in (0..self.rows.len()).rev() {
            if matches!(self.rows.get(i), Some(PickerRow::Item { .. })) {
                self.cursor = i;
                self.follow_scroll();
                return;
            }
        }
    }

    // ── 搜索 ──────────────────────────────────────────────────────────────────

    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.cursor = 0;
        self.scroll_top = 0;
        self.rebuild_rows();
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.cursor = 0;
        self.scroll_top = 0;
        self.rebuild_rows();
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.cursor = 0;
        self.scroll_top = 0;
        self.rebuild_rows();
    }

    /// 粘贴文本到搜索过滤器。
    pub fn append_filter(&mut self, s: &str) {
        self.filter.push_str(s);
        self.cursor = 0;
        self.scroll_top = 0;
        self.rebuild_rows();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    // ── 选择 ──────────────────────────────────────────────────────────────────

    /// 选中的模型引用 "provider/model"。
    pub fn selected_ref(&self) -> Option<String> {
        match self.rows.get(self.cursor) {
            Some(PickerRow::Item { model_idx }) => {
                let m = &self.all_models[*model_idx];
                Some(format!("{}/{}", m.provider_id, m.id))
            }
            _ => None,
        }
    }

    // ── 布局 ──────────────────────────────────────────────────────────────────

    pub fn height(&self) -> u16 {
        if self.all_models.is_empty() {
            return 4;
        }
        // 标题(1) + 提示(1) + 可见行 + 滚动指示
        let visible = self.rows.len().min(MAX_VISIBLE);
        let mut h = 2 + visible;
        if self.scroll_top > 0 {
            h += 1;
        }
        if self.scroll_top + MAX_VISIBLE < self.rows.len() {
            h += 1;
        }
        h as u16
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

    // ── 渲染 ──────────────────────────────────────────────────────────────────

    pub fn render(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);
        let header_style = Style::default()
            .fg(UI)
            .add_modifier(Modifier::BOLD);

        if self.all_models.is_empty() {
            return Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(
                        "Switch Model",
                        Style::default().fg(UI).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  —  no models", Style::default().fg(UI)),
                ]),
                Line::from(Span::styled("No models available. Run /connect first.", dim)),
                Line::from(Span::styled("Esc to cancel", dim)),
            ]);
        }

        let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
            Span::styled(
                "Switch Model",
                Style::default().fg(UI).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  —  {} models", self.all_models.len()),
                Style::default().fg(UI),
            ),
        ])];

        // 搜索行
        if self.filter.is_empty() {
            lines.push(Line::from(Span::styled(
                "type to search  •  ↑↓ navigate  PgUp/Dn  Enter select  Esc cancel",
                dim,
            )));
        } else {
            let item_count = self
                .rows
                .iter()
                .filter(|r| matches!(r, PickerRow::Item { .. }))
                .count();
            lines.push(Line::from(vec![
                Span::styled("filter: ", dim),
                Span::styled(self.filter.clone(), Style::default().fg(WARN)),
                Span::styled(format!("  ({})  Enter select  Esc cancel", item_count), dim),
            ]));
        }

        // 上方滚动指示
        if self.scroll_top > 0 {
            lines.push(Line::from(Span::styled(
                format!("  ↑ {} more", self.scroll_top),
                dim,
            )));
        }

        let end = (self.scroll_top + MAX_VISIBLE).min(self.rows.len());
        for i in self.scroll_top..end {
            match &self.rows[i] {
                PickerRow::Header {
                    provider_name,
                    count,
                } => {
                    lines.push(Line::from(vec![
                        Span::styled(format!("── {} ", provider_name), header_style),
                        Span::styled(format!("({})", count), dim),
                        Span::styled(" ──", header_style),
                    ]));
                }
                PickerRow::Item { model_idx } => {
                    let m = &self.all_models[*model_idx];
                    let selected = i == self.cursor;
                    let active = format!("{}/{}", m.provider_id, m.id) == self.active_ref;

                    let pointer = if selected { "❯ " } else { "  " };
                    let name = display_name(m);

                    let mut spans = vec![
                        Span::styled(
                            pointer,
                            Style::default().fg(UI).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            name,
                            if selected {
                                Style::default().fg(UI).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(INACTIVE)
                            },
                        ),
                    ];

                    // role 标签
                    if let Some(tag) = role_tag(classify_model(&m.id)) {
                        spans.push(Span::styled(format!(" [{}]", tag), dim));
                    }

                    // 当前模型标记
                    if active {
                        spans.push(Span::styled(" ◀", Style::default().fg(OK)));
                    }

                    lines.push(Line::from(spans));
                }
            }
        }

        // 下方滚动指示
        if end < self.rows.len() {
            lines.push(Line::from(Span::styled(
                format!("  ↓ {} more", self.rows.len() - end),
                dim,
            )));
        }

        Paragraph::new(lines)
    }

    pub fn clear() -> Clear {
        Clear
    }
}
