use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

use crate::tui::theme::{mode_color, ACCENT, MAIN, MUTED, UI};

fn mode_desc(mode: &str) -> &'static str {
    match mode {
        "build"  => "all tools auto-approved",
        "edit"   => "file edits auto-approved, no shell",
        "plan"   => "read/explore allowed, writes/bash ask",
        "ask"    => "read-only, no writes",
        "agent"  => "fully autonomous, no permission checks",
        _        => "",
    }
}

/// 欢迎屏的行数（供布局顶部对齐时测高）。
pub fn welcome_height(model: &str, mode: &str, cwd: &str) -> usize {
    welcome_lines(model, mode, cwd).len()
}

pub fn render_welcome<'a>(model: &'a str, mode: &'a str, cwd: &'a str) -> Paragraph<'a> {
    Paragraph::new(welcome_lines(model, mode, cwd)).wrap(Wrap { trim: false })
}

fn welcome_lines<'a>(model: &'a str, mode: &'a str, cwd: &'a str) -> Vec<Line<'a>> {
    let mcolor = mode_color(mode);
    let mdesc  = mode_desc(mode);

    let mut lines: Vec<Line<'a>> = Vec::new();

    // ── cat + header ──────────────────────────────────────────────────────────
    // 猫占 3 行，右侧信息同行排列
    lines.push(Line::from(vec![
        Span::styled("  /\\  /\\   ", Style::default().fg(UI)),
        Span::styled("✻ ", Style::default().fg(ACCENT)),
        Span::styled("Welcome to NekoCLI", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" v{VERSION}"), Style::default().fg(MUTED)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" ( o  o )  ", Style::default().fg(UI)),
        Span::styled("Model   ", Style::default().fg(MUTED)),
        Span::styled(model, Style::default().fg(MAIN)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  \\ ^^ /   ", Style::default().fg(UI)),
        Span::styled("Mode    ", Style::default().fg(MUTED)),
        Span::styled(mode.to_uppercase(), Style::default().fg(mcolor).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {}", mdesc), Style::default().fg(MUTED)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("           ", Style::default()),
        Span::styled("CWD     ", Style::default().fg(MUTED)),
        Span::styled(cwd, Style::default().fg(MAIN)),
    ]));

    lines.push(Line::from(""));

    // ── Quick start ───────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        "  Quick start",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let tips: &[(&str, &str)] = &[
        ("Tab",             "cycle mode: ask → edit → plan → build → agent"),
        ("↑ / ↓",          "browse input history"),
        ("@file.ts",        "attach file or directory to message"),
        ("Ctrl+A / Ctrl+E", "line start / end"),
        ("Ctrl+C",          "clear input, or press twice to exit"),
    ];
    for (key, desc) in tips {
        lines.push(Line::from(vec![
            Span::styled(format!("    {:<22}", key), Style::default().fg(UI)),
            Span::styled(*desc, Style::default().fg(MUTED)),
        ]));
    }

    lines.push(Line::from(""));

    // ── Commands ──────────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        "  Commands  (/ to see all)",
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let cmds: &[(&str, &str)] = &[
        ("/help",     "full command list"),
        ("/model",    "switch model"),
        ("/sessions", "list saved sessions"),
        ("/memory",   "manage persistent memory"),
        ("/clear",    "clear the screen"),
        ("/quit",     "exit neko"),
    ];
    for chunk in cmds.chunks(2) {
        let mut spans = vec![Span::raw("    ")];
        for (name, desc) in chunk {
            spans.push(Span::styled(format!("{:<12}", name), Style::default().fg(UI)));
            spans.push(Span::styled(format!("{:<28}", desc), Style::default().fg(MUTED)));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));

    // ── Separator ─────────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        format!("  {}", "─".repeat(52)),
        Style::default().fg(MUTED),
    )));
    lines.push(Line::from(Span::styled(
        "  Start typing to chat  ·  / for commands  ·  Tab to switch mode",
        Style::default().fg(MUTED),
    )));

    lines
}
