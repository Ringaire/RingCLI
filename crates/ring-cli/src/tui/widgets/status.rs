use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::tui::theme::{mode_color, SPINNER, ERR, WARN, MUTED};

/// Render the status bar.
///
/// - Left  : `MODE  model` (mode colored)
/// - Center: spinner + "working" when running
/// - Right : `~N/200k (pct%)  Tab:mode`
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub fn render_status(
    mode:           &str,
    model:          &str,
    cwd:            &str,
    tokens:         u64,
    context_window: u64,
    is_running:     bool,
    spinner_idx:    usize,
    skip_perms:     bool,
    tasks:          (usize, usize, usize),
    area_width:     u16,
) -> Paragraph<'static> {
    let mcolor = mode_color(mode);

    // ── left part ─────────────────────────────────────────────────────────────
    let mode_str  = format!(" {}", mode.to_uppercase());
    let model_str = format!("  {}", model);

    // ── center (running indicator) ────────────────────────────────────────────
    let center_str = if is_running {
        format!("  {}  working", SPINNER[spinner_idx % SPINNER.len()])
    } else if skip_perms {
        "  SKIP-PERM".to_string()
    } else {
        String::new()
    };

    // ── right part ────────────────────────────────────────────────────────────
    let (right_str, right_color) = if tokens > 0 && context_window > 0 {
        let pct = tokens as f64 / context_window as f64 * 100.0;
        let s = format!(
            "~{}/{:.0}k ({:.1}%)  Tab:mode ",
            tokens,
            context_window as f64 / 1000.0,
            pct,
        );
        let c = if pct >= 85.0 {
            ERR
        } else if pct >= 70.0 {
            WARN
        } else {
            MUTED
        };
        (s, c)
    } else {
        ("Tab:mode ".to_string(), MUTED)
    };

    // ── cwd basename ──────────────────────────────────────────────────────────
    let cwd_base = cwd.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(cwd);
    let cwd_str  = format!("  {}", cwd_base);

    // ── tasks summary ───────────────────────────────────────────────────────────
    let (pending, in_progress, done) = tasks;
    let task_str = if pending + in_progress + done > 0 {
        format!("✔{} ◼{} ◻{}  ", done, in_progress, pending)
    } else {
        String::new()
    };

    // ── padding ───────────────────────────────────────────────────────────────
    let left_w   = mode_str.width() + model_str.width() + cwd_str.width() + center_str.width();
    let right_w  = task_str.width() + right_str.width();
    let total    = area_width as usize;
    let pad      = total.saturating_sub(left_w + right_w);

    // ── build spans ───────────────────────────────────────────────────────────
    let mut spans = vec![
        Span::styled(mode_str,  Style::default().fg(mcolor).add_modifier(Modifier::BOLD)),
        Span::styled(model_str, Style::default().fg(MUTED)),
        Span::styled(cwd_str,   Style::default().fg(MUTED)),
    ];

    if !center_str.is_empty() {
        let center_color = if skip_perms { ERR } else { MUTED };
        spans.push(Span::styled(center_str, Style::default().fg(center_color)));
    }

    spans.push(Span::raw(" ".repeat(pad)));
    if !task_str.is_empty() {
        spans.push(Span::styled(task_str, Style::default().fg(MUTED)));
    }
    spans.push(Span::styled(right_str, Style::default().fg(right_color)));

    Paragraph::new(Line::from(spans))
}
