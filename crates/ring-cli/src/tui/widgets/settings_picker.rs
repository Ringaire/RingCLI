//! 设置面板：聚合会话级偏好设置。
//!
//! `/setting` 无参数时触发。boolean 项 Enter 直接 toggle，
//! enum 项（effort/mode）Enter 打开对应子选择器。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::tui::theme::{INACTIVE, MUTED, OK, UI};

/// 设置项类型（决定 Enter 时的行为）。
#[derive(Clone, Copy, PartialEq)]
pub enum SettingKind {
    /// 切换 extended thinking 开关。
    ToggleThinking,
    /// 切换 reasoning 显示。
    ToggleShowThinking,
    /// 切换 token 计数显示。
    ToggleTokenCount,
    /// 打开 effort 选择器。
    OpenEffort,
    /// 打开权限模式选择器。
    OpenMode,
}

struct SettingEntry {
    label: &'static str,
    kind: SettingKind,
}

const ENTRIES: &[SettingEntry] = &[
    SettingEntry { label: "Effort",           kind: SettingKind::OpenEffort },
    SettingEntry { label: "Permission Mode",  kind: SettingKind::OpenMode },
    SettingEntry { label: "Extended Thinking",kind: SettingKind::ToggleThinking },
    SettingEntry { label: "Show Thinking",    kind: SettingKind::ToggleShowThinking },
    SettingEntry { label: "Token Counter",    kind: SettingKind::ToggleTokenCount },
];

/// 设置当前值快照（构造时传入，toggle 时同步更新）。
#[derive(Clone)]
pub struct SettingsSnapshot {
    pub effort:           String,
    pub mode:             String,
    pub think_enabled:    bool,
    pub think_show:       bool,
    pub show_token_count: bool,
}

pub struct SettingsPickerModal {
    snap:   SettingsSnapshot,
    cursor: usize,
}

impl SettingsPickerModal {
    pub fn new(snap: SettingsSnapshot) -> Self {
        Self { snap, cursor: 0 }
    }

    pub fn snapshot(&self) -> &SettingsSnapshot {
        &self.snap
    }

    pub fn current_kind(&self) -> SettingKind {
        ENTRIES[self.cursor].kind
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < ENTRIES.len() {
            self.cursor += 1;
        }
    }

    /// toggle 一个 boolean 项（同步更新快照）。
    pub fn toggle(&mut self, kind: SettingKind) {
        match kind {
            SettingKind::ToggleThinking     => self.snap.think_enabled    = !self.snap.think_enabled,
            SettingKind::ToggleShowThinking => self.snap.think_show       = !self.snap.think_show,
            SettingKind::ToggleTokenCount   => self.snap.show_token_count = !self.snap.show_token_count,
            _ => {}
        }
    }

    pub fn height(&self) -> u16 {
        2 + ENTRIES.len() as u16
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
                "Settings",
                Style::default().fg(UI).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  —  ↑↓ navigate · Enter toggle/open · Esc close", dim),
        ])];

        for (i, e) in ENTRIES.iter().enumerate() {
            let selected = i == self.cursor;
            let pointer = if selected { "❯ " } else { "  " };
            let label_style = if selected {
                Style::default().fg(UI).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(INACTIVE)
            };

            // 当前值显示
            let (value, value_style) = match e.kind {
                SettingKind::OpenEffort => {
                    let v = if self.snap.effort.is_empty() { "off" } else { &self.snap.effort };
                    (v.to_string(), Style::default().fg(UI))
                }
                SettingKind::OpenMode => {
                    (self.snap.mode.clone(), Style::default().fg(UI))
                }
                SettingKind::ToggleThinking => {
                    let on = self.snap.think_enabled;
                    (if on { "ON" } else { "OFF" }.into(),
                     if on { Style::default().fg(OK) } else { dim })
                }
                SettingKind::ToggleShowThinking => {
                    let on = self.snap.think_show;
                    (if on { "show" } else { "hide" }.into(),
                     if on { Style::default().fg(OK) } else { dim })
                }
                SettingKind::ToggleTokenCount => {
                    let on = self.snap.show_token_count;
                    (if on { "ON" } else { "OFF" }.into(),
                     if on { Style::default().fg(OK) } else { dim })
                }
            };

            lines.push(Line::from(vec![
                Span::styled(pointer, Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<18}", e.label), label_style),
                Span::styled(value, value_style),
            ]));
        }

        Paragraph::new(lines)
    }

    pub fn clear() -> Clear {
        Clear
    }
}
