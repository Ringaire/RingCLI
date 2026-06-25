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

use crate::tui::theme::{UI, MUTED, ERR, INACTIVE};

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
    /// OpenAI 专属：选择认证方式（ChatGPT OAuth / API Key）。
    SelectAuthMethod,
    ApiKey,
    CustomName,
    CustomUrl,
    CustomKey,
    FetchingModels,
    /// OAuth2 浏览器/设备码流程进行中。
    OAuthInProgress,
    SelectModel,
    /// 模型列表为空时，手动输入模型 ID。
    ManualModel,
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
    /// app.rs 须启动 ChatGPT OAuth2 浏览器登录流程。
    OAuthBrowser,
    /// app.rs 须启动 ChatGPT OAuth2 设备码登录流程。
    OAuthDevice,
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

    /// 手动输入的模型 ID（模型列表为空时）。
    manual_model: String,

    /// `SelectAuthMethod` 步骤的 cursor（0=browser, 1=device, 2=apikey）。
    auth_cursor: usize,

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
            manual_model: String::new(),
            auth_cursor: 0,
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
        if !self.manual_model.is_empty() {
            return Some(self.manual_model.clone());
        }
        self.models.get(self.model_list.cursor()).cloned().or_else(|| self.default_model.clone())
    }

    /// 落盘后回传的模型缓存（拉到的完整列表，可空）。
    pub fn fetched_models(&self) -> &[String] { &self.models }

    // ── app.rs 回传：异步结果 ────────────────────────────────────────────────

    /// 模型列表拉取完成。非空 → 进入选择；空 → 直接落盘（用默认模型）。
    pub fn apply_models(&mut self, models: Vec<String>) -> SetupAction {
        if models.is_empty() {
            self.text.clear();
            self.step = SetupStep::ManualModel;
            SetupAction::Stay
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

    /// OAuth2 登录结果。成功 → 用 API key 进入模型拉取；失败 → 显示错误。
    pub fn apply_oauth_result(&mut self, result: Result<String, String>) {
        match result {
            Ok(api_key) => {
                self.api_key = api_key;
                self.text.clear();
                self.step = SetupStep::FetchingModels;
                // app.rs 看到 Fetch 后会构造 probe provider 拉模型
            }
            Err(e) => {
                self.status = e;
                self.step = SetupStep::Error;
            }
        }
    }

    // ── 按键 ─────────────────────────────────────────────────────────────────

    pub fn move_up(&mut self) {
        match self.step {
            SetupStep::SelectProvider => self.prov_list.up(self.filtered_indices().len()),
            SetupStep::SelectAuthMethod => {
                if self.auth_cursor > 0 { self.auth_cursor -= 1; }
            }
            SetupStep::SelectModel    => self.model_list.up(self.models.len()),
            _ => {}
        }
    }

    pub fn move_down(&mut self) {
        match self.step {
            SetupStep::SelectProvider => self.prov_list.down(self.filtered_indices().len()),
            SetupStep::SelectAuthMethod => {
                if self.auth_cursor < 2 { self.auth_cursor += 1; }
            }
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

    /// 粘贴文本（bracketed paste）。
    pub fn insert_str(&mut self, s: &str) {
        if self.is_text_step() { self.text.push_str(s); }
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
            SetupStep::SelectProvider
                | SetupStep::ApiKey
                | SetupStep::CustomName
                | SetupStep::CustomUrl
                | SetupStep::CustomKey
                | SetupStep::ManualModel
        )
    }

    /// `SelectProvider` 步骤中，根据 `text` 过滤后的 provider 在原始列表中的索引。
    /// `text` 为空 → 全部。Custom 行始终保留在末尾。
    fn filtered_indices(&self) -> Vec<usize> {
        let q = self.text.to_lowercase();
        if q.is_empty() {
            return (0..self.providers.len()).collect();
        }
        self.providers
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.is_custom()
                    || r.id.to_lowercase().contains(&q)
                    || r.name.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Enter：推进状态机。返回 app.rs 须执行的外部动作。
    pub fn confirm(&mut self) -> SetupAction {
        match self.step {
            SetupStep::SelectProvider => {
                let indices = self.filtered_indices();
                let &orig_idx = indices.get(self.prov_list.cursor()).unwrap_or(&0);
                let Some(row) = self.providers.get(orig_idx).cloned() else {
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
                    // OpenAI 专属：进入认证方式子菜单
                    if row.id == "openai" {
                        self.auth_cursor = 0;
                        self.step = SetupStep::SelectAuthMethod;
                        SetupAction::Stay
                    } else if row.needs_key() {
                        self.step = SetupStep::ApiKey;
                        SetupAction::Stay
                    } else {
                        // 无需 key（Ollama / LM Studio）：直接拉模型
                        self.step = SetupStep::FetchingModels;
                        SetupAction::Fetch
                    }
                }
            }
            SetupStep::SelectAuthMethod => {
                match self.auth_cursor {
                    0 => { // ChatGPT browser
                        self.step = SetupStep::OAuthInProgress;
                        SetupAction::OAuthBrowser
                    }
                    1 => { // ChatGPT device code
                        self.step = SetupStep::OAuthInProgress;
                        SetupAction::OAuthDevice
                    }
                    2 => { // API Key
                        self.step = SetupStep::ApiKey;
                        SetupAction::Stay
                    }
                    _ => SetupAction::Stay,
                }
            }
            SetupStep::OAuthInProgress => SetupAction::Stay,
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
            SetupStep::ManualModel => {
                let m = self.text.trim().to_string();
                if m.is_empty() { return SetupAction::Stay; }
                self.manual_model = m;
                self.text.clear();
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
            SetupStep::SelectAuthMethod => {
                self.step = SetupStep::SelectProvider;
                self.text.clear();
                SetupAction::Stay
            }
            SetupStep::ApiKey | SetupStep::CustomName | SetupStep::SelectModel | SetupStep::ManualModel | SetupStep::Error => {
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
            SetupStep::FetchingModels | SetupStep::Saving | SetupStep::OAuthInProgress => SetupAction::Stay,
        }
    }

    // ── 渲染 ─────────────────────────────────────────────────────────────────

    pub fn height(&self) -> u16 {
        match self.step {
            SetupStep::SelectProvider => {
                let len = self.filtered_indices().len();
                // 标题(1) + 搜索框(1) + 提示(1) + 列表
                self.prov_list.body_height(len, 3)
            }
            SetupStep::SelectModel    => self.model_list.body_height(self.models.len(), 2),
            SetupStep::SelectAuthMethod => 6, // 标题(1) + 3选项(3) + 空行(1) + 提示(1)
            SetupStep::OAuthInProgress => 3,
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
            SetupStep::SelectAuthMethod => self.render_auth_method(),
            SetupStep::SelectModel    => self.render_model_list(),
            SetupStep::ManualModel    => self.render_input("Enter Model", &self.provider_name(), "Model ID", false, "no models fetched — type a model ID  Enter to confirm  Esc to cancel"),
            SetupStep::ApiKey         => self.render_input("Connect", &self.provider_name(), "API Key", true, "Enter to fetch models  Esc to go back"),
            SetupStep::CustomName     => self.render_input("Custom Provider", "step 1 / 3", "Name", false, "Enter to continue  Esc to cancel"),
            SetupStep::CustomUrl      => self.render_input("Custom Provider", "step 2 / 3", "Base URL", false, "Enter to continue  Esc to go back"),
            SetupStep::CustomKey      => self.render_input("Custom Provider", "step 3 / 3", "API Key", true, "Enter to skip / continue  Esc to go back"),
            SetupStep::FetchingModels => Self::render_status("Fetching models…", UI, "please wait"),
            SetupStep::OAuthInProgress => Self::render_status("OAuth2 in progress…", UI, "complete login in your browser  Esc to cancel"),
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

    fn render_auth_method(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);
        let options: &[(&str, &str)] = &[
            ("ChatGPT Login (browser)",    "OAuth2 PKCE — Pro/Plus subscription"),
            ("ChatGPT Login (device code)", "Headless — SSH / no-browser"),
            ("API Key",                     "sk-... direct key input"),
        ];

        let mut lines: Vec<Line<'static>> = vec![
            Line::from(vec![
                Span::styled("Connect OpenAI", Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled("  —  choose authentication", dim),
            ]),
        ];

        for (i, (name, desc)) in options.iter().enumerate() {
            let selected = i == self.auth_cursor;
            let pointer = if selected { "❯ " } else { "  " };
            let name_style = if selected {
                Style::default().fg(UI).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(INACTIVE)
            };

            lines.push(Line::from(vec![
                Span::styled(pointer, Style::default().fg(UI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<28}", name), name_style),
                Span::styled(*desc, dim),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑↓ navigate  Enter select  Esc back",
            dim,
        )));

        Paragraph::new(lines)
    }

    fn render_provider_list(&self) -> Paragraph<'static> {
        let dim = Style::default().fg(MUTED);
        let indices = self.filtered_indices();
        let filtered: Vec<&ProviderRow> = indices.iter()
            .filter_map(|&i| self.providers.get(i))
            .collect();

        let mut lines: Vec<Line<'static>> = vec![
            Line::from(Span::styled("Connect Provider", Style::default().fg(UI).add_modifier(Modifier::BOLD))),
        ];

        // 搜索框
        lines.push(Line::from(vec![
            Span::styled("filter: ", dim),
            Span::styled(self.text.clone(), Style::default().fg(UI)),
            Span::styled("▏", Style::default().fg(UI)),
        ]));

        // 提示行
        if self.text.is_empty() {
            lines.push(Line::from(Span::styled(
                "type to search  •  ↑↓ navigate  Enter select  Esc cancel",
                dim,
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{} match(es)  •  ↑↓ navigate  Enter select  Esc cancel", filtered.len()),
                dim,
            )));
        }

        lines.extend(self.prov_list.render_rows(&filtered, dim, |row, rs| {
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
