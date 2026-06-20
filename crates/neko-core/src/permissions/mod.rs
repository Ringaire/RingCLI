use serde::{Deserialize, Serialize};

// ── 类型 ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeName {
    /// 全自动：bash / 文件写入 / 编辑均自动放行，无弹框。
    #[default]
    Auto,
    /// 编辑模式：文件写入 / 编辑自动放行，bash 禁止。
    Edit,
    /// 询问模式（只读）：bash / 写入 / 编辑均拒绝。
    Ask,
    /// 绕过：跳过整个权限引擎，一切放行（含 custom deny 规则）。
    Bypass,
}

impl std::str::FromStr for ModeName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto"   => Ok(Self::Auto),
            "edit"   => Ok(Self::Edit),
            "ask"    => Ok(Self::Ask),
            "bypass" => Ok(Self::Bypass),
            // 向后兼容旧配置文件里的 "build"
            "build"  => Ok(Self::Auto),
            other    => Err(format!("unknown mode: {other}")),
        }
    }
}

impl std::fmt::Display for ModeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto   => write!(f, "auto"),
            Self::Edit   => write!(f, "edit"),
            Self::Ask    => write!(f, "ask"),
            Self::Bypass => write!(f, "bypass"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub tool: String,
    pub path: Option<String>,
    pub action: PermissionAction,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AccessCheck<'a> {
    pub tool: &'a str,
    pub path: Option<&'a str>,
    pub description: &'a str,
    pub preview: Option<&'a str>,
}

// ── Glob 匹配 ─────────────────────────────────────────────────────────────────

fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return value.ends_with(suffix);
    }
    pattern == value
}

fn rule_matches(rule: &PermissionRule, req: &AccessCheck<'_>) -> bool {
    if !glob_match(&rule.tool, req.tool) {
        return false;
    }
    if let (Some(rule_path), Some(req_path)) = (&rule.path, req.path) {
        if !glob_match(rule_path, req_path) {
            return false;
        }
    }
    true
}

// ── 模式默认规则 ──────────────────────────────────────────────────────────────

/// auto：全自动放行，不弹任何确认框。
fn auto_rules() -> Vec<PermissionRule> {
    vec![
        PermissionRule { tool: "*".into(), path: None, action: PermissionAction::Allow, reason: None },
    ]
}

/// edit：文件写入 / 编辑自动放行；bash 禁止。
fn edit_rules() -> Vec<PermissionRule> {
    vec![
        PermissionRule { tool: "bash".into(), path: None, action: PermissionAction::Deny,  reason: Some("shell disabled in edit mode".into()) },
        PermissionRule { tool: "*".into(),     path: None, action: PermissionAction::Allow, reason: None },
    ]
}

/// ask：只读模式，bash / 写入 / 编辑全部拒绝。
fn ask_rules() -> Vec<PermissionRule> {
    let allow = |tool: &str| PermissionRule { tool: tool.into(), path: None, action: PermissionAction::Allow, reason: None };
    vec![
        PermissionRule { tool: "bash".into(),      path: None, action: PermissionAction::Deny, reason: Some("read-only mode".into()) },
        PermissionRule { tool: "write_file".into(), path: None, action: PermissionAction::Deny, reason: Some("read-only mode".into()) },
        PermissionRule { tool: "edit_file".into(),  path: None, action: PermissionAction::Deny, reason: Some("read-only mode".into()) },
        allow("lsp_diagnostics"),
        allow("lsp_refs"),
        allow("read_file"),
        allow("glob"),
        allow("grep"),
        allow("web_fetch"),
        allow("web_search"),
        allow("token_count"),
        allow("todo"),
        PermissionRule { tool: "*".into(), path: None, action: PermissionAction::Deny, reason: Some("read-only mode".into()) },
    ]
}

// ── 权限引擎 ──────────────────────────────────────────────────────────────────

pub struct DefaultPermissionEngine {
    mode:     ModeName,
    custom:   Vec<PermissionRule>,
    skip_all: bool,
}

impl DefaultPermissionEngine {
    pub fn new(mode: ModeName) -> Self {
        Self { mode, custom: Vec::new(), skip_all: false }
    }

    pub fn mode(&self) -> ModeName {
        self.mode
    }

    pub fn is_permissions_skipped(&self) -> bool {
        self.skip_all || self.mode == ModeName::Bypass
    }

    pub fn set_mode(&mut self, mode: ModeName) {
        self.mode = mode;
    }

    pub fn dangerously_skip_permissions(&mut self) {
        self.skip_all = true;
    }

    pub fn allow(&mut self, tool: impl Into<String>, path: Option<String>) {
        let tool = tool.into();
        self.remove_existing(&tool, path.as_deref());
        self.custom.insert(0, PermissionRule { tool, path, action: PermissionAction::Allow, reason: None });
    }

    pub fn deny(&mut self, tool: impl Into<String>, path: Option<String>) {
        let tool = tool.into();
        self.remove_existing(&tool, path.as_deref());
        self.custom.insert(0, PermissionRule { tool, path, action: PermissionAction::Deny, reason: None });
    }

    pub fn evaluate(&self, req: &AccessCheck<'_>) -> PermissionAction {
        // bypass 和 skip_all 都跳过一切检查
        if self.skip_all || self.mode == ModeName::Bypass {
            return PermissionAction::Allow;
        }
        for rule in &self.custom {
            if rule_matches(rule, req) {
                return rule.action;
            }
        }
        let mode_rules = match self.mode {
            ModeName::Auto   => auto_rules(),
            ModeName::Edit   => edit_rules(),
            ModeName::Ask    => ask_rules(),
            ModeName::Bypass => unreachable!(),
        };
        for rule in &mode_rules {
            if rule_matches(rule, req) {
                return rule.action;
            }
        }
        PermissionAction::Ask
    }

    pub fn custom_rules(&self) -> &[PermissionRule] {
        &self.custom
    }

    fn remove_existing(&mut self, tool: &str, path: Option<&str>) {
        self.custom.retain(|r| !(r.tool == tool && r.path.as_deref() == path));
    }
}
