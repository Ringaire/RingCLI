use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::session::paths;

// ── Provider 配置 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key:  Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

// ── MCP 服务器配置 ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

// ── Session 配置 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    pub max_messages: usize,
    pub max_tokens:   u64,
    pub auto_save_ms: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { max_messages: 200, max_tokens: 180_000, auto_save_ms: 0 }
    }
}

// ── UI 配置 ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiConfig {
    pub theme:            Theme,
    pub compact_mode:     bool,
    pub show_token_count: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { theme: Theme::Auto, compact_mode: false, show_token_count: true }
    }
}

// ── 用户配置（未解析，允许部分字段缺失）────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NekoUserConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model:       Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub providers:   Option<HashMap<String, ProviderEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models:      Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy:       Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<HashMap<String, McpServerConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session:     Option<SessionConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui:          Option<UiConfig>,
}

// ── 解析后的配置（合并三层后的最终结果）────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub model:       Option<String>,
    pub providers:   HashMap<String, ProviderEntry>,
    pub models:      HashMap<String, Vec<String>>,
    pub proxy:       Option<String>,
    pub mcp_servers: HashMap<String, McpServerConfig>,
    pub session:     SessionConfig,
    pub ui:          UiConfig,
    pub config_path: PathBuf,
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        Self {
            model:       None,
            providers:   HashMap::new(),
            models:      HashMap::new(),
            proxy:       None,
            mcp_servers: HashMap::new(),
            session:     SessionConfig::default(),
            ui:          UiConfig::default(),
            config_path: paths::config_path(),
        }
    }
}

// ── JSONC 解析（去除注释）────────────────────────────────────────────────────

fn strip_jsonc_comments(src: &str) -> String {
    let mut out  = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if !in_string => { in_string = true;  out.push(ch); }
            '"' if in_string  => { in_string = false; out.push(ch); }
            '\\' if in_string => {
                out.push(ch);
                if let Some(next) = chars.next() { out.push(next); }
            }
            '/' if !in_string => {
                match chars.peek() {
                    Some('/') => { while chars.next().is_some_and(|c| c != '\n') {} out.push('\n'); }
                    Some('*') => {
                        chars.next();
                        loop {
                            match chars.next() {
                                Some('*') if chars.peek() == Some(&'/') => { chars.next(); break; }
                                None => break,
                                _ => {}
                            }
                        }
                    }
                    _ => out.push(ch),
                }
            }
            _ => out.push(ch),
        }
    }
    out
}

fn parse_jsonc<T: for<'de> Deserialize<'de>>(src: &str) -> Result<T, serde_json::Error> {
    let stripped = strip_jsonc_comments(src);
    serde_json::from_str(&stripped)
}

// ── 深度合并 ──────────────────────────────────────────────────────────────────

fn merge_config(base: &mut NekoUserConfig, over: NekoUserConfig) {
    if let Some(m) = over.model       { base.model       = Some(m); }
    if let Some(p) = over.proxy       { base.proxy        = Some(p); }
    if let Some(ps) = over.providers  {
        let target = base.providers.get_or_insert_with(HashMap::new);
        for (k, v) in ps { target.insert(k, v); }
    }
    if let Some(ms) = over.models {
        let target = base.models.get_or_insert_with(HashMap::new);
        for (k, v) in ms { target.insert(k, v); }
    }
    if let Some(mcp) = over.mcp_servers {
        let target = base.mcp_servers.get_or_insert_with(HashMap::new);
        for (k, v) in mcp { target.insert(k, v); }
    }
    if let Some(s) = over.session { base.session = Some(s); }
    if let Some(u) = over.ui      { base.ui      = Some(u); }
}

// ── Claude Code 兼容：.mcp.json 读取 ──────────────────────────────────────────

/// 从 cwd 向上遍历，收集所有 `.mcp.json` 文件路径。
///
/// 返回顺序为**远→近**（父目录在前，cwd 在后），使加载时近的覆盖远的，
/// 与 Claude Code 的 `config.ts:909-961` 遍历逻辑对齐。
fn find_mcp_json_files(cwd: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut current = Some(cwd);
    while let Some(dir) = current {
        let path = dir.join(".mcp.json");
        if path.is_file() {
            files.push(path);
        }
        current = dir.parent();
    }
    files.reverse(); // 近→远 反转为 远→近
    files
}

/// 展开 `${VAR}` 和 `$VAR` 环境变量引用。
///
/// 对照 CC `config.ts:556-616`：缺失变量保留原样（不阻塞），仅 stdio 的
/// command/args/env 做展开。
fn expand_env_vars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            // ${VAR} 形式
            if bytes[i + 1] == b'{' {
                if let Some(end) = s[i + 2..].find('}') {
                    let var = &s[i + 2..i + 2 + end];
                    match std::env::var(var) {
                        Ok(val) => out.push_str(&val),
                        Err(_) => out.push_str(&s[i..i + 2 + end + 1]), // 保留原样
                    }
                    i += 2 + end + 1;
                    continue;
                }
            }
            // $VAR 形式（字母/下划线开头，字母数字下划线续接）
            if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                let var = &s[start..end];
                match std::env::var(var) {
                    Ok(val) => out.push_str(&val),
                    Err(_) => out.push_str(&s[i..end]),
                }
                i = end;
                continue;
            }
        }
        // 安全的 UTF-8 边界推进
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// 返回 UTF-8 起始字节对应的字符长度。
fn utf8_char_len(byte: u8) -> usize {
    if byte < 0x80 { 1 }
    else if byte < 0xC0 { 1 } // 续接字节（不应出现在起始位置，容错）
    else if byte < 0xE0 { 2 }
    else if byte < 0xF0 { 3 }
    else { 4 }
}

/// 递归地对 JSON Value 中的所有字符串做环境变量展开。
fn expand_env_in_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => { *s = expand_env_vars(s); }
        serde_json::Value::Array(arr) => { for v in arr { expand_env_in_value(v); } }
        serde_json::Value::Object(map) => { for (_, v) in map.iter_mut() { expand_env_in_value(v); } }
        _ => {}
    }
}

/// 解析 `.mcp.json` 文件内容。
///
/// - 预处理缺省 `type` 字段：无 `type` → 默认 `"stdio"`（兼容 CC）
/// - 展开 `${VAR}` / `$VAR` 环境变量（command/args/env）
/// - 返回 server name → config 映射
fn parse_mcp_json(raw: &str) -> HashMap<String, McpServerConfig> {
    let mut value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };

    if let Some(servers) = value.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        for (_, server) in servers.iter_mut() {
            // 无 type → 默认 stdio
            if server.get("type").is_none() {
                if let Some(obj) = server.as_object_mut() {
                    obj.insert("type".into(), serde_json::Value::String("stdio".into()));
                }
            }
            // 环境变量展开
            expand_env_in_value(server);
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Wrapper {
        mcp_servers: HashMap<String, McpServerConfig>,
    }

    serde_json::from_value::<Wrapper>(value)
        .map(|w| w.mcp_servers)
        .unwrap_or_default()
}

// ── 加载配置 ──────────────────────────────────────────────────────────────────

pub async fn load_config(cwd: Option<&std::path::Path>) -> ResolvedConfig {
    let mut merged = NekoUserConfig::default();

    // 1. 全局配置 (~/.config/neko/settings.jsonc)
    let global_path = paths::config_path();
    if let Ok(raw) = tokio::fs::read_to_string(&global_path).await {
        if let Ok(cfg) = parse_jsonc::<NekoUserConfig>(&raw) {
            merge_config(&mut merged, cfg);
        }
    }

    // 2. Claude Code 兼容：从 cwd 向上遍历 .mcp.json（远→近，近覆盖远）
    if let Some(dir) = cwd {
        for path in find_mcp_json_files(dir) {
            if let Ok(raw) = tokio::fs::read_to_string(&path).await {
                let mcp = parse_mcp_json(&raw);
                if !mcp.is_empty() {
                    let target = merged.mcp_servers.get_or_insert_with(HashMap::new);
                    for (k, v) in mcp {
                        target.insert(k, v);
                    }
                }
            }
        }
    }

    // 3. 项目配置 (.neko/settings.jsonc)
    if let Some(dir) = cwd {
        let project_path = dir.join(".neko").join("settings.jsonc");
        if let Ok(raw) = tokio::fs::read_to_string(&project_path).await {
            if let Ok(cfg) = parse_jsonc::<NekoUserConfig>(&raw) {
                merge_config(&mut merged, cfg);
            }
        }
        // 4. 本地覆盖（不提交）
        let local_path = dir.join(".neko").join("settings.local.jsonc");
        if let Ok(raw) = tokio::fs::read_to_string(&local_path).await {
            if let Ok(cfg) = parse_jsonc::<NekoUserConfig>(&raw) {
                merge_config(&mut merged, cfg);
            }
        }
    }

    let mut providers = merged.providers.unwrap_or_default();
    inject_env_keys(&mut providers);

    // 代理：config 未设置时读环境变量
    let proxy = merged.proxy.or_else(read_proxy_env);

    ResolvedConfig {
        model:       merged.model,
        providers,
        models:      merged.models.unwrap_or_default(),
        proxy,
        mcp_servers: merged.mcp_servers.unwrap_or_default(),
        session:     merged.session.unwrap_or_default(),
        ui:          merged.ui.unwrap_or_default(),
        config_path: global_path,
    }
}

// ── 读写全局用户配置（未解析，供向导/CLI 修改后回写）──────────────────────────

/// 只读取**全局** `settings.jsonc` 的原始用户配置（不注入环境变量、不合并项目层）。
///
/// 用于 `/connect` 向导等需要修改并回写配置的场景——回写 `ResolvedConfig` 会把环境变量里的
/// API key 持久化进文件，因此修改前必须从这里拿到磁盘上的原始形态。文件缺失/解析失败时返回默认值。
pub async fn load_user_config() -> NekoUserConfig {
    let path = paths::config_path();
    match tokio::fs::read_to_string(&path).await {
        Ok(raw) => parse_jsonc::<NekoUserConfig>(&raw).unwrap_or_default(),
        Err(_)  => NekoUserConfig::default(),
    }
}

/// 将用户配置写回全局 `settings.jsonc`（pretty JSON）。
///
/// 先写同目录临时文件再 rename，保证原子性。父目录不存在则创建。
/// **注意**：JSONC 注释会在回写时丢失（与 bun `saveConfig` 行为一致）。
pub async fn save_config(cfg: &NekoUserConfig) -> std::io::Result<()> {
    let path = paths::config_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let bytes = serde_json::to_vec_pretty(cfg)?;
    let tmp = path.with_extension("jsonc.tmp");
    tokio::fs::write(&tmp, &bytes).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(())
}

// ── 环境变量注入 ──────────────────────────────────────────────────────────────

/// provider id → 对应的环境变量名（API key）
fn env_var_for_provider(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "anthropic"  => Some("ANTHROPIC_API_KEY"),
        "openai"     => Some("OPENAI_API_KEY"),
        "gemini"     => Some("GEMINI_API_KEY"),
        "deepseek"   => Some("DEEPSEEK_API_KEY"),
        "groq"       => Some("GROQ_API_KEY"),
        "mistral"    => Some("MISTRAL_API_KEY"),
        "together"   => Some("TOGETHER_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "xai"        => Some("XAI_API_KEY"),
        "moonshot"   => Some("MOONSHOT_API_KEY"),
        _            => None,
    }
}

/// provider id → base_url 环境变量名（覆盖默认 endpoint）
fn base_url_env_for_provider(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "anthropic" => Some("ANTHROPIC_BASE_URL"),
        "openai"    => Some("OPENAI_BASE_URL"),
        "gemini"    => Some("GEMINI_BASE_URL"),
        "ollama"    => Some("OLLAMA_BASE_URL"),
        _           => None,
    }
}

/// 已知 provider id 列表（用于扫描环境变量，自动补全 config 未声明但有 env key 的 provider）
const KNOWN_PROVIDERS: &[&str] = &[
    "anthropic", "openai", "gemini", "deepseek", "groq",
    "mistral", "together", "openrouter", "xai", "moonshot", "ollama",
];

/// 对每个 provider：若 config 中 api_key/base_url 缺失，尝试从环境变量补全。
/// 此外，扫描已知 provider，若环境变量存在 API key 但 config 未声明，自动创建条目。
fn inject_env_keys(providers: &mut HashMap<String, ProviderEntry>) {
    // 已声明的 provider：补全缺失字段
    for (id, entry) in providers.iter_mut() {
        if entry.api_key.is_none() {
            if let Some(var) = env_var_for_provider(id) {
                if let Ok(val) = std::env::var(var) {
                    if !val.trim().is_empty() {
                        entry.api_key = Some(val);
                    }
                }
            }
        }
        if entry.base_url.is_none() {
            if let Some(var) = base_url_env_for_provider(id) {
                if let Ok(val) = std::env::var(var) {
                    if !val.trim().is_empty() {
                        entry.base_url = Some(val);
                    }
                }
            }
        }
    }

    // 未声明但有 env key 的 provider：自动补建
    for &id in KNOWN_PROVIDERS {
        if providers.contains_key(id) {
            continue;
        }
        let api_key = env_var_for_provider(id)
            .and_then(|var| std::env::var(var).ok())
            .filter(|v| !v.trim().is_empty());
        let base_url = base_url_env_for_provider(id)
            .and_then(|var| std::env::var(var).ok())
            .filter(|v| !v.trim().is_empty());

        if api_key.is_some() || base_url.is_some() {
            providers.insert(id.to_string(), ProviderEntry { api_key, base_url });
        }
    }
}

/// 从标准代理环境变量读取（HTTPS_PROXY 优先于 HTTP_PROXY，大小写都尝试）
fn read_proxy_env() -> Option<String> {
    for var in &["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
        if let Ok(val) = std::env::var(var) {
            if !val.trim().is_empty() {
                return Some(val);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mcp_json_explicit_stdio() {
        let raw = r#"{
            "mcpServers": {
                "fs": {
                    "type": "stdio",
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem"],
                    "env": { "ROOT": "/tmp" }
                }
            }
        }"#;
        let mcp = parse_mcp_json(raw);
        assert_eq!(mcp.len(), 1);
        match mcp.get("fs").unwrap() {
            McpServerConfig::Stdio { command, args, env } => {
                assert_eq!(command, "npx");
                assert_eq!(args, &vec!["-y", "@modelcontextprotocol/server-filesystem"]);
                assert_eq!(env.get("ROOT").map(|s| s.as_str()), Some("/tmp"));
            }
            _ => panic!("expected Stdio"),
        }
    }

    #[test]
    fn parse_mcp_json_implicit_stdio() {
        // CC 格式：无 type 字段 → 默认 stdio
        let raw = r#"{
            "mcpServers": {
                "fs": {
                    "command": "npx",
                    "args": ["-y", "@mcp/server"]
                }
            }
        }"#;
        let mcp = parse_mcp_json(raw);
        assert_eq!(mcp.len(), 1);
        assert!(matches!(mcp.get("fs").unwrap(), McpServerConfig::Stdio { .. }));
    }

    #[test]
    fn parse_mcp_json_sse() {
        let raw = r#"{
            "mcpServers": {
                "remote": {
                    "type": "sse",
                    "url": "https://example.com/mcp",
                    "headers": { "Authorization": "Bearer token" }
                }
            }
        }"#;
        let mcp = parse_mcp_json(raw);
        assert_eq!(mcp.len(), 1);
        match mcp.get("remote").unwrap() {
            McpServerConfig::Sse { url, headers } => {
                assert_eq!(url, "https://example.com/mcp");
                assert_eq!(headers.get("Authorization").map(|s| s.as_str()), Some("Bearer token"));
            }
            _ => panic!("expected Sse"),
        }
    }

    #[test]
    fn parse_mcp_json_mixed_servers() {
        let raw = r#"{
            "mcpServers": {
                "local": { "command": "node", "args": ["server.js"] },
                "remote": { "type": "sse", "url": "https://example.com/mcp" }
            }
        }"#;
        let mcp = parse_mcp_json(raw);
        assert_eq!(mcp.len(), 2);
        assert!(matches!(mcp.get("local").unwrap(), McpServerConfig::Stdio { .. }));
        assert!(matches!(mcp.get("remote").unwrap(), McpServerConfig::Sse { .. }));
    }

    #[test]
    fn parse_mcp_json_env_expansion() {
        std::env::set_var("NEKO_TEST_MCP_CMD", "expanded-cmd");
        std::env::set_var("NEKO_TEST_MCP_KEY", "secret123");
        let raw = r#"{
            "mcpServers": {
                "test": {
                    "command": "${NEKO_TEST_MCP_CMD}",
                    "args": ["--key", "$NEKO_TEST_MCP_KEY"],
                    "env": { "TOKEN": "${NEKO_TEST_MCP_KEY}" }
                }
            }
        }"#;
        let mcp = parse_mcp_json(raw);
        match mcp.get("test").unwrap() {
            McpServerConfig::Stdio { command, args, env } => {
                assert_eq!(command, "expanded-cmd");
                assert_eq!(args, &vec!["--key", "secret123"]);
                assert_eq!(env.get("TOKEN").map(|s| s.as_str()), Some("secret123"));
            }
            _ => panic!("expected Stdio"),
        }
    }

    #[test]
    fn parse_mcp_json_missing_var_preserved() {
        let raw = r#"{
            "mcpServers": {
                "test": {
                    "command": "${NEKO_NONEXISTENT_VAR}",
                    "args": []
                }
            }
        }"#;
        let mcp = parse_mcp_json(raw);
        match mcp.get("test").unwrap() {
            McpServerConfig::Stdio { command, .. } => {
                // 缺失变量保留原样
                assert_eq!(command, "${NEKO_NONEXISTENT_VAR}");
            }
            _ => panic!("expected Stdio"),
        }
    }

    #[test]
    fn parse_mcp_json_invalid_returns_empty() {
        let mcp = parse_mcp_json("not json at all");
        assert!(mcp.is_empty());
    }

    #[test]
    fn parse_mcp_json_empty_servers() {
        let raw = r#"{ "mcpServers": {} }"#;
        let mcp = parse_mcp_json(raw);
        assert!(mcp.is_empty());
    }

    #[test]
    fn expand_env_vars_brace_form() {
        std::env::set_var("NEKO_TEST_EXPAND", "hello");
        assert_eq!(expand_env_vars("${NEKO_TEST_EXPAND}"), "hello");
        assert_eq!(expand_env_vars("pre-${NEKO_TEST_EXPAND}-post"), "pre-hello-post");
    }

    #[test]
    fn expand_env_vars_dollar_form() {
        std::env::set_var("NEKO_TEST_DOLLAR", "world");
        assert_eq!(expand_env_vars("$NEKO_TEST_DOLLAR"), "world");
        assert_eq!(expand_env_vars("x-$NEKO_TEST_DOLLAR-y"), "x-world-y");
    }

    #[test]
    fn expand_env_vars_no_var_preserved() {
        assert_eq!(expand_env_vars("plain text"), "plain text");
        assert_eq!(expand_env_vars("${NEKO_DOES_NOT_EXIST}"), "${NEKO_DOES_NOT_EXIST}");
    }
}
