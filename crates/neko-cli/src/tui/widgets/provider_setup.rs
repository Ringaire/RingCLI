//! `/connect` 向导：选 provider → 填 key → 拉 /models → 选模型。
//!
//! **纯 UI / 状态机，无任何 IO**。网络拉取与配置落盘由 app.rs 驱动——
//! widget 仅在按键时推进可推进的状态，并通过 [`SetupAction`] 告知 app.rs 何时需要外部动作。
//! 对照 bun `ProviderSetup.tsx`，风格沿用 `model_picker.rs`。

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use neko_providers::ProviderKind;

use crate::tui::theme::{UI, MUTED, ERR};

use super::core::scroll_list::{anchor_below, label, pointer, ScrollList};

const VISIBLE:       usize = 9;
const MODEL_VISIBLE: usize = 10;

/// 自定义 provider 在列表里的哨兵 id。
pub const CUSTOM_ID: &str = "__custom__";

// ── provider 列表行（由 app.rs 从 catalog 构造后传入）─────────────────────────

#[derive(Clone)]
pub struct ProviderRow {
    pub id:            String,
    pub name:          String,
    pub kind:          ProviderKind,
    pub base_url:      Option<String>,
    pub api_key_env:   Option<String>,
    pub default_model: Option<String>,
}

impl ProviderRow {
    /// 末尾的"自定义端点"行。
    pub fn custom() -> Self {
        Self {
            id:            CUSTOM_ID.to_string(),
            name:          "Custom  (OpenAI-compatible)".to_string(),
            kind:          ProviderKind::OpenAiCompatible,
            base_url:      None,
            api_key_env:   None,
            default_model: None,
        }
    }

    fn is_custom(&self) -> bool {
        self.id == CUSTOM_ID
    }

    /// 需要 API key（preset 声明了 api_key_env）。
    fn needs_key(&self) -> bool {
        self.api_key_env.is_some()
    }

    fn hint(&self) -> String {
        if self.is_custom() {
            "enter name, url, key".to_string()
        } else if let Some(env) = &self.api_key_env {
            format!("env: {env}")
        } else {
            "no key needed".to_string()
        }
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    SelectProvider,
    ApiKey,
    CustomName,
    CustomUrl,
    CustomKey,
    FetchingModels,
    SelectModel,
    Saving,
    Error,
}

/// app.rs 在 `handle_key` / `apply_models` 后需要执行的外部动作。
pub enum SetupAction {
    /// 仅重绘，无外部动作。
    Stay,
    /// 关闭向导（用户取消）。
    Cancel,
    /// app.rs 须构造 probe provider 调 `list_models`，结果回传 [`ProviderSetupModal::apply_models`]。
    Fetch,
    /// app.rs 须落盘配置 + 热重载，结果回传 [`ProviderSetupModal::apply_saved`]。
    Commit,
}

// ── 模态 ──────────────────────────────────────────────────────────────────────

pub struct ProviderSetupModal {
    step:        SetupStep,
    providers:   Vec<ProviderRow>,
    prov_list:   ScrollList,

    /// 共享文本输入缓冲（换步时清空）。
    text:        String,

    /// 选定/录入的 provider 身份。
    provider_id: String,
    is_custom:   bool,
    kind:        ProviderKind,
    base_url:    Option<String>,
    default_model: Option<String>,
    api_key:     String,

    /// 模型选择。
    models:      Vec<String>,
    model_list:  ScrollList,

    status: String,
}

impl ProviderSetupModal {
    /// `providers` 应已包含末尾的 [`ProviderRow::custom`]。
    pub fn new(providers: Vec<ProviderRow>) -> Self {
        Self {
            step:        SetupStep::SelectProvider,
            providers,
            prov_list:   ScrollList::new(VISIBLE),
            text:        String::new(),
            provider_id: String::new(),
            is_custom:   false,
            kind:        ProviderKind::OpenAiCompatible,
            base_url:    None,
            default_model: None,
            api_key:     String::new(),
            models:      Vec::new(),
            model_list:  ScrollList::new(MODEL_VISIBLE),
            status:      String::new(),
        }
    }

    // ── app.rs 读取：驱动 Fetch / Commit 所需的字段 ──────────────────────────

    pub fn provider_id(&self) -> &str { &self.provider_id }
    pub fn kind(&self) -> ProviderKind { self.kind.clone() }
    pub fn base_url(&self) -> Option<String> { self.base_url.clone() }
    pub fn api_key(&self) -> &str { &self.api_key }
    pub fn is_custom(&self) -> bool { self.is_custom }
    pub fn default_model(&self) -> Option<String> { self.default_model.clone() }

    /// 最终选定的模型：SelectModel 选中项，否则回退 default_model。
    pub fn chosen_model(&self) -> Option<String> {
        self.models.get(self.model_list.cursor()).cloned().or_else(|| self.default_model.clone())
    }

    /// 落盘后回传的模型缓存（拉到的完整列表，可空）。
    pub fn fetched_models(&self) -> &[String] { &self.models }

    // ── app.rs 回传：异步结果 ────────────────────────────────────────────────

    /// 模型列表拉取完成。非空 → 进入选择；空 → 直接落盘（用默认模型）。
    pub fn apply_models(&mut self, models: Vec<String>) -> SetupAction {
        if models.is_empty() {
            self.step = SetupStep::Saving;
            SetupAction::Commit
        } else {
            self.models = models;
            self.model_list = ScrollList::new(MODEL_VISIBLE);
            self.step = SetupStep::SelectModel;
            SetupAction::Stay
        }
    }

    /// 模型拉取失败。
    pub fn fetch_failed(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.step = SetupStep::Error;
    }

    /// 落盘结果。Ok → app.rs 自行关闭并推送 system 消息；Err → 显示错误。
    pub fn apply_saved(&mut self, result: Result<(), String>) {
        if let Err(e) = result {
            self.status = e;
            self.step = SetupStep::Error;
        }
    }

    // ── 按键 ─────────────────────────────────────────────────────────────────

    pub fn move_up(&mut self) {
        match self.step {
            SetupStep::SelectProvider => self.prov_list.up(self.providers.len()),
            SetupStep::SelectModel    => self.model_list.up(self.models.len()),
            _ => {}
        }
    }

    pub fn move_down(&mut self) {
        match self.step {
            SetupStep::SelectProvider => self.prov_list.down(self.providers.len()),
            SetupStep::SelectModel    => self.model_list.down(self.models.len()),
            _ => {}
        }
    }

    /// 文本步：录入字符。
    pub fn input_char(&mut self, c: char) {
        if self.is_text_step() { self.text.push(c); }
    }

    pub fn backspace(&mut self) {
        if self.is_text_step() { self.text.pop(); }
    }

    /// Ctrl+U 清空，Ctrl+W 删词。
    pub fn clear_line(&mut self) {
        if self.is_text_step() { self.text.clear(); }
    }

    pub fn delete_word(&mut self) {
        if !self.is_text_step() { return; }
        let trimmed = self.text.trim_end();
        match trimmed.rfind(char::is_whitespace) {
            Some(i) => self.text.truncate(i + 1),
            None    => self.text.clear(),
        }
    }

    fn is_text_step(&self) -> bool {
        matches!(
            self.step,
            SetupStep::ApiKey | SetupStep::CustomName | SetupStep::CustomUrl | SetupStep::CustomKey
        )
    }

    /// Enter：推进状态机。返回 app.rs 须执行的外部动作。
    pub fn confirm(&mut self) -> SetupAction {
        match self.step {
            SetupStep::SelectProvider => {
                let Some(row) = self.providers.get(self.prov_list.cursor()).cloned() else {
                    return SetupAction::Stay;
                };
                if row.is_custom() {
                    self.is_custom = true;
                    self.kind = ProviderKind::OpenAiCompatible;
                    self.text.clear();
                    self.step = SetupStep::CustomName;
                    SetupAction::Stay
                } else {
                    self.is_custom = false;
                    self.provider_id   = row.id.clone();
                    self.kind          = row.kind.clone();
                    self.base_url      = row.base_url.clone();
                    self.default_model = row.default_model.clone();
                    self.text.clear();
                    if row.needs_key() {
                        self.step = SetupStep::ApiKey;
                        SetupAction::Stay
                    } else {
                        // 无需 key（Ollama / LM Studio）：直接拉模型
                        self.step = SetupStep::FetchingModels;
                        SetupAction::Fetch
                    }
                }
            }
            SetupStep::ApiKey => {
                self.api_key = self.text.trim().to_string();
                self.text.clear();
                self.step = SetupStep::FetchingModels;
                SetupAction::Fetch
            }
            SetupStep::CustomName => {
                let name = self.text.trim().to_string();
                if name.is_empty() { return SetupAction::Stay; }
                self.provider_id = name;
                self.text.clear();
                self.step = SetupStep::CustomUrl;
                SetupAction::Stay
            }
            SetupStep::CustomUrl => {
                let url = self.text.trim().to_string();
                if url.is_empty() { return SetupAction::Stay; }
                self.base_url = Some(url);
                self.text.clear();
                self.step = SetupStep::CustomKey;
                SetupAction::Stay
            }
            SetupStep::CustomKey => {
                self.api_key = self.text.trim().to_string();
                self.text.clear();
                self.step = SetupStep::FetchingModels;
                SetupAction::Fetch
            }
            SetupStep::SelectModel => {
                self.step = SetupStep::Saving;
                SetupAction::Commit
            }
            SetupStep::Error => {
                // 回到 provider 选择重来
                self.step = SetupStep::SelectProvider;
                self.text.clear();
                SetupAction::Stay
            }
            SetupStep::FetchingModels | SetupStep::Saving => SetupAction::Stay,
        }
    }

    /// Esc：回退一步或取消。
    pub fn cancel(&mut self) -> SetupAction {
        match self.step {
            SetupStep::SelectProvider => SetupAction::Cancel,
            SetupStep::ApiKey | SetupStep::CustomName | SetupStep::SelectModel | SetupStep::Error => {
                self.step = SetupStep::SelectProvider;
                self.text.clear();
                SetupAction::Stay
            }
            SetupStep::CustomUrl => {
                self.text = self.provider_id.clone();
                self.step = SetupStep::CustomName;
                SetupAction::Stay
            }
            SetupStep::CustomKey => {
                self.text = self.base_url.clone().unwrap_or_default();
                self.step = SetupStep::CustomUrl;
                SetupAction::Stay
            }
            SetupStep::FetchingModels | SetupStep::Saving => SetupAction::Stay,
        }
    }

    // ── 渲染 ─────────────────────────────────────────────────────────────────

    pub fn height(&self) -> u16 {
        match self.step {
            SetupStep::SelectProvider => self.prov_list.body_height(self.providers.len(), 2),
            SetupStep::SelectModel    => self.model_list.body_height(self.models.len(), 2),
            _ => 4,
        }
    }

    pub fn area(parent: Rect, y: u16, h: u16) -> Rect {
        anchor_below(parent, y, h)
    }

    pub fn clear() -> Clear { Clear }

    pub fn render(&self) -> Paragraph<'static> {
        match self.step {
            SetupStep::SelectProvider => self.render_provider_list(),
            SetupStep::SelectModel    => self.render_model_list(),
            SetupStep::ApiKey         => self.render_input("Connect", &self.provider_name(), "API Key", true, "Enter to fetch models  Esc to go back"),
            SetupStep::CustomName     => self.render_input("Custom Provider", "step 1 / 3", "Name", false, "Enter to continue  Esc to cancel"),
            SetupStep::CustomUrl      => self.render_input("Custom Provider", "step 2 / 3", "Base URL", false, "Enter to continue  Esc to go back"),
            SetupStep::CustomKey      => self.render_input("Custom Provider", "step 3 / 3", "API Key", true, "Enter to skip / continue  Esc to go back"),
            SetupStep::FetchingModels => Self::render_status("Fetching models…", UI, "please wait"),
            SetupStep::Saving         => Self::render_status("Saving…", UI, ""),
            SetupStep::Error          => Self::render_status(&format!("✗ {}", self.status), ERR, "Enter to retry  Esc to cancel"),
        }
    }

    fn provider_name(&self) -> String {
        if self.is_custom { self.provider_id.clone() } else {
            self.providers.iter().find(|r| r.id == self.provider_id)
                .map(|r| r.name.clone()).unwrap_or_else(|| self.provider_id.clone())
        }
    }

    fn render_provider_list(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);
        let mut lines: Vec<Line<'static>> = vec![
            Line::from(Span::styled("Connect Provider", Style::default().fg(UI).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled("↑↓ navigate  Enter select  Esc cancel", dim)),
        ];
        lines.extend(self.prov_list.render_rows(&self.providers, dim, |row, rs| {
            Line::from(vec![
                pointer(rs.selected),
                label(rs.selected, format!("{:<14}", row.id)),
                label(rs.selected, format!("{:<26}", row.name)),
                Span::styled(row.hint(), dim),
            ])
        }));
        Paragraph::new(lines)
    }

    fn render_model_list(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);
        let mut lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                Span::styled("Select Model", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  —  {}", self.provider_id), Style::default().fg(UI)),
            ]),
            Line::from(Span::styled(
                format!("{} models  •  ↑↓ navigate  Enter select  Esc back", self.models.len()),
                dim,
            )),
        ];
        lines.extend(self.model_list.render_rows(&self.models, dim, |id, rs| {
            Line::from(vec![pointer(rs.selected), label(rs.selected, id.clone())])
        }));
        Paragraph::new(lines)
    }

    fn render_input(
        &self,
        title:    &str,
        subtitle: &str,
        label:    &str,
        mask:     bool,
        footer:   &str,
    ) -> Paragraph<'static> {
        let shown = if mask { mask_key(&self.text) } else { self.text.clone() };
        let lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                Span::styled(title.to_string(), Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {subtitle}"), Style::default().fg(MUTED)),
            ]),
            Line::from(vec![
                Span::styled(format!("{:<10}", format!("{label}:")), Style::default().fg(MUTED)),
                Span::styled(shown, Style::default().fg(UI)),
                Span::styled("▏", Style::default().fg(UI)),
            ]),
            Line::from(Span::styled(footer.to_string(), Style::default().fg(MUTED))),
        ];
        Paragraph::new(lines)
    }

    fn render_status(msg: &str, color: Color, footer: &str) -> Paragraph<'static> {
        let mut lines = vec![Line::from(Span::styled(msg.to_string(), Style::default().fg(color)))];
        if !footer.is_empty() {
            lines.push(Line::from(Span::styled(footer.to_string(), Style::default().fg(MUTED))));
        }
        Paragraph::new(lines)
    }
}

/// 掩码显示 API key：保留前 6 位，其余以 • 替代（最多 20 个）。仿 bun `maskKey`。
fn mask_key(key: &str) -> String {
    let n = key.chars().count();
    if n == 0 { return String::new(); }
    if n <= 8 { return "•".repeat(n); }
    let head: String = key.chars().take(6).collect();
    let dots = (n - 6).min(20);
    format!("{head}{}", "•".repeat(dots))
}
