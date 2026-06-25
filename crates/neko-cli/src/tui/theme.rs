//! TUI 共享主题常量与样式 helper（集中单一来源，避免各 widget 复制）。

use ratatui::style::Color;

/// 运行中 spinner 帧。
pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── 语义颜色 ──────────────────────────────────────────────────────────────

/// AI 强调色（橙色）。
pub const ACCENT: Color = Color::Rgb(214, 142, 104);
/// UI 界面色（青）— 指针、标签、边框、快捷键。
pub const UI: Color = Color::Cyan;
/// 成功 / 完成（绿）。
pub const OK: Color = Color::Green;
/// 警告 / 编辑模式（黄）。
pub const WARN: Color = Color::Yellow;
/// 错误 / 危险（红）。
pub const ERR: Color = Color::Red;
/// 推理 / 子 agent（紫）。
pub const THINK: Color = Color::Magenta;
/// 正文（白）。
pub const MAIN: Color = Color::White;
/// 次要 / 提示（暗灰）。
pub const MUTED: Color = Color::DarkGray;
/// 禁用 / 非活跃（灰）。
pub const INACTIVE: Color = Color::Gray;

/// 权限模式 → 颜色。
pub fn mode_color(mode: &str) -> Color {
    match mode {
        "build"  => OK,
        "edit"   => WARN,
        "plan"   => Color::Cyan,
        "ask"    => Color::Blue,
        "agent"  => ERR,
        // 向后兼容
        "auto"   => OK,
        "bypass" => ERR,
        _        => MAIN,
    }
}
