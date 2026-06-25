// Provider 工厂：从 ResolvedConfig + Catalog 构建 ProviderRegistry

use std::collections::HashMap;
use std::path::PathBuf;

use tracing::{debug, info, warn};

use neko_core::config::ResolvedConfig;
use neko_core::session::paths;

use crate::catalog::{self, CatalogEntry, ProviderKind};
use crate::provider::{build_http_client, DEFAULT_CONNECT_TIMEOUT_SECS};
use crate::providers::{
    anthropic::AnthropicProvider,
    anthropic::claude_code::ClaudeCodeProvider,
    compatible::CompatibleProvider,
    gemini::GeminiProvider,
    openai::OpenAiProvider,
};
use crate::registry::ProviderRegistry;

/// 工厂构建结果：注册表 + 推断出的默认 provider id
pub struct ProviderBootstrap {
    pub registry:            ProviderRegistry,
    pub default_provider_id: Option<String>,
    /// models.dev 缓存路径（供调用方按需刷新）
    pub models_cache_path:   PathBuf,
}

/// 从配置构建 provider 注册表。
///
/// 1. 加载 catalog（内置 JSON + 用户覆盖文件，三层合并）
/// 2. 以 `config.providers` 为驱动——只注册用户显式配置了 key/url 的 provider
/// 3. 遍历 catalog 中"不需要 key"的 provider（如 Ollama）自动注册
pub fn build_registry(config: &ResolvedConfig) -> ProviderBootstrap {
    let client = build_http_client(config.proxy.as_deref(), DEFAULT_CONNECT_TIMEOUT_SECS);

    // 加载 catalog（内置 + 全局 + 项目）
    let global_dir = paths::config_dir();
    let project_dir = std::env::current_dir().ok();
    let cat = catalog::load(
        Some(&global_dir),
        project_dir.as_deref(),
    );

    let mut registry = ProviderRegistry::new();

    // ── 1. 注册用户在 config.providers 里显式声明的 provider ─────────────────
    for (id, user_entry) in &config.providers {
        let cat_entry = match catalog::get(&cat, id) {
            Some(e) => e.clone(),
            None => {
                // 用户声明了一个 catalog 里没有的 provider：必须提供 base_url
                let Some(base_url) = user_entry.base_url.clone() else {
                    warn!(provider = %id, "unknown provider and no base_url, skipping");
                    continue;
                };
                CatalogEntry {
                    name: id.clone(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: Some(base_url),
                    api_key_env: None,
                    default_model: None,
                    extra_body: None,
                }
            }
        };

        // 解析 api_key：config 优先，其次 env var
        let api_key = user_entry.api_key.clone()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| cat_entry.api_key_env.as_deref().and_then(|env| std::env::var(env).ok()))
            .unwrap_or_default();

        // base_url：config 优先，其次 catalog 默认
        let base_url = user_entry.base_url.clone().or_else(|| cat_entry.base_url.clone());

        let needs_key = cat_entry.api_key_env.is_some();
        if needs_key && api_key.is_empty() {
            warn!(provider = %id, "skipping: no API key");
            continue;
        }

        register_one(&mut registry, &client, id, &cat_entry.kind, api_key, base_url, cat_entry.default_model.clone(), cat_entry.extra_body.clone());
    }

    // ── 2. 自动注册无需 key 的 catalog provider（Ollama / LM Studio）─────────
    for (id, entry) in &cat {
        if registry.contains(id) { continue; }
        if entry.api_key_env.is_some() { continue; } // 需要 key，跳过

        let base_url = entry.base_url.clone();
        register_one(&mut registry, &client, id, &entry.kind, String::new(), base_url, entry.default_model.clone(), entry.extra_body.clone());
        debug!(provider = %id, "auto-registered keyless provider");
    }

    // ── 3. 自动检测 claude CLI → 注册 claude-code provider ──────────────────
    if !registry.contains("claude-code") {
        if let Some(cc) = ClaudeCodeProvider::detect() {
            registry.register(cc);
            info!("auto-registered claude-code provider (claude CLI detected)");
        }
    }

    let default_provider_id = pick_default(&registry, config);
    if let Some(ref d) = default_provider_id {
        info!(provider = %d, "default provider selected");
    } else {
        warn!("no usable provider configured");
    }

    ProviderBootstrap {
        registry,
        default_provider_id,
        models_cache_path: paths::cache_dir().join("models.json"),
    }
}

fn register_one(
    registry:      &mut ProviderRegistry,
    client:        &reqwest::Client,
    id:            &str,
    kind:          &ProviderKind,
    api_key:       String,
    base_url:      Option<String>,
    default_model: Option<String>,
    extra_body:    Option<serde_json::Value>,
) {
    match kind {
        ProviderKind::Anthropic => {
            // Anthropic provider 使用内置默认模型常量，不接收 def_model 参数。
            registry.register(AnthropicProvider::with_client(
                client.clone(), api_key, base_url,
            ));
            debug!(provider = %id, "registered anthropic");
        }
        ProviderKind::OpenAi => {
            registry.register(OpenAiProvider::with_client(
                client.clone(), api_key, base_url, None, default_model,
            ));
            debug!(provider = %id, "registered openai");
        }
        ProviderKind::Gemini => {
            registry.register(GeminiProvider::with_client(
                client.clone(), api_key, base_url, default_model,
            ));
            debug!(provider = %id, "registered gemini");
        }
        ProviderKind::OpenAiCompatible => {
            let base = base_url.unwrap_or_else(|| {
                warn!(provider = %id, "openai-compatible provider has no base_url");
                String::new()
            });
            let def_model = default_model.unwrap_or_default();
            registry.register(CompatibleProvider::with_client_and_extra(
                client.clone(),
                id.to_string(),
                id.to_string(),
                api_key,
                base,
                def_model,
                extra_body,
            ));
            debug!(provider = %id, "registered openai-compatible");
        }
    }
}

/// 选择默认 provider —— **仅当用户有明确意图时**才返回，否则 None（冷启动 → Setup Required）。
///
/// 意图信号：(1) `config.model` 显式指定；(2) `config.providers` 里声明（含 env key 注入的条目）。
/// keyless 自动注册的 provider（Ollama / LM Studio）若未被上述显式选中，**不**作为默认——
/// 避免无配置时静默连到本地端点，对照计划的 provider-neutral 门控。
fn pick_default(registry: &ProviderRegistry, config: &ResolvedConfig) -> Option<String> {
    // 1. config.model 显式指定的 provider（覆盖 ollama 等本地端点的主动选择）
    if let Some(model) = &config.model {
        if let Some((prov, _)) = model.split_once('/') {
            if registry.contains(prov) {
                return Some(prov.to_string());
            }
        }
    }

    // 2. 用户在 config.providers 里声明且成功注册的 provider（含 env key 注入项），按优先序
    const PRIORITY: &[&str] = &[
        "anthropic", "openai", "gemini", "deepseek", "groq",
        "mistral", "together", "openrouter", "xai", "moonshot",
    ];
    for &id in PRIORITY {
        if config.providers.contains_key(id) && registry.contains(id) {
            return Some(id.to_string());
        }
    }
    for id in config.providers.keys() {
        if registry.contains(id) { return Some(id.clone()); }
    }

    // 3. claude-code 兜底：有 claude CLI 但无 API key 时自动选择
    if registry.contains("claude-code") && config.providers.is_empty() && config.model.is_none() {
        return Some("claude-code".to_string());
    }

    // 4. 无任何显式配置 → 不预设 provider
    None
}

/// 构建单个临时 provider（不进任何长期注册表），供 `/connect` 向导探测 `/models` 列表使用。
///
/// `proxy` 透传到 HTTP 客户端；`kind`/`base_url`/`default_model` 通常取自 catalog 条目，
/// 自定义端点则由调用方直接给出。
pub fn build_probe_provider(
    proxy:         Option<&str>,
    kind:          &ProviderKind,
    id:            &str,
    api_key:       String,
    base_url:      Option<String>,
    default_model: Option<String>,
) -> Option<std::sync::Arc<dyn crate::provider::Provider>> {
    let client = build_http_client(proxy, DEFAULT_CONNECT_TIMEOUT_SECS);
    let mut registry = ProviderRegistry::new();
    register_one(&mut registry, &client, id, kind, api_key, base_url, default_model, None);
    registry.get(id)
}

// ── 向后兼容的辅助函数 ────────────────────────────────────────────────────────

/// 解析 "provider/model" 字符串。
pub fn split_model_ref(model: &str) -> (Option<String>, String) {
    match model.split_once('/') {
        Some((prov, m)) => (Some(prov.to_string()), m.to_string()),
        None            => (None, model.to_string()),
    }
}

/// 返回 catalog 中所有已知的 provider id。
pub fn known_provider_ids() -> Vec<String> {
    catalog::defaults().into_keys().collect()
}

/// 调试用：打印 config 里的 provider 配置摘要。
pub fn summarize(config: &ResolvedConfig) -> HashMap<String, bool> {
    config.providers.iter()
        .map(|(id, e)| (id.clone(), e.api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false)))
        .collect()
}
