// Provider 工厂：从 ResolvedConfig + Catalog 构建 ProviderRegistry

use std::collections::HashMap;
use std::path::PathBuf;

use tracing::{debug, info, warn};

use ring_core::config::ResolvedConfig;
use ring_core::session::paths;

use crate::catalog::{self, ProviderKind};
use crate::provider::{build_http_client, DEFAULT_CONNECT_TIMEOUT_SECS};
use crate::providers::{
    anthropic::AnthropicProvider,
    anthropic::claude_code::ClaudeCodeProvider,
    compatible::CompatibleProvider,
    gemini::GeminiProvider,
    openai::OpenAiProvider,
    openai::responses::OpenAiResponsesProvider,
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
/// 2. 遍历 catalog，注册所有有 API key 或无需 key 的 provider
/// 3. 自动检测 claude CLI
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

    // ── 1. 遍历 catalog，注册所有可用的 provider ───────────────────────────
    for (id, entry) in &cat {
        // 解析 api_key：catalog 直接提供，或从 env 读取
        let api_key = entry.api_key.clone()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| entry.api_key_env.as_deref().and_then(|env| std::env::var(env).ok()))
            .unwrap_or_default();

        // 内置 provider 需要 key 但没有提供 → 跳过
        // 用户自定义 provider（有 api_key 字段）即使没有 api_key_env 也注册
        let is_builtin_needs_key = entry.api_key_env.is_some();
        if is_builtin_needs_key && api_key.is_empty() {
            debug!(provider = %id, "skipping builtin provider: no API key");
            continue;
        }

        debug!(provider = %id, api_key_len = api_key.len(), "registering from catalog");
        register_one(RegisterParams {
            registry: &mut registry,
            client: &client,
            id,
            kind: &entry.kind,
            api_key,
            base_url: entry.base_url.clone(),
            default_model: entry.default_model.clone(),
            extra_body: entry.extra_body.clone(),
        });
    }

    // ── 2. 自动检测 claude CLI → 注册 claude-code provider ──────────────────
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

struct RegisterParams<'a> {
    registry:      &'a mut ProviderRegistry,
    client:        &'a reqwest::Client,
    id:            &'a str,
    kind:          &'a ProviderKind,
    api_key:       String,
    base_url:      Option<String>,
    default_model: Option<String>,
    extra_body:    Option<serde_json::Value>,
}

fn register_one(params: RegisterParams) {
    let RegisterParams {
        registry,
        client,
        id,
        kind,
        api_key,
        base_url,
        default_model,
        extra_body,
    } = params;
    
    match kind {
        ProviderKind::Anthropic => {
            registry.register(AnthropicProvider::with_client_as(
                client.clone(), id, id, api_key, base_url,
            ));
            debug!(provider = %id, "registered anthropic");
        }
        ProviderKind::OpenAi => {
            registry.register(OpenAiProvider::with_client(
                client.clone(), api_key, base_url, None, default_model,
            ));
            debug!(provider = %id, "registered openai");
        }
        ProviderKind::OpenAiResponses => {
            let mut provider = OpenAiResponsesProvider::with_client(
                client.clone(), api_key, base_url, None, default_model,
            );
            if let Some(extra) = extra_body {
                provider = provider.with_extra_body(extra);
            }
            registry.register(provider);
            debug!(provider = %id, "registered openai-responses");
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

/// 选择默认 provider：优先 config.model 指定的 provider，其次按优先级从已注册 provider 中选择。
fn pick_default(registry: &ProviderRegistry, config: &ResolvedConfig) -> Option<String> {
    // 1. config.model 显式指定的 provider
    if let Some(model) = &config.model {
        if let Some((prov, _)) = model.split_once('/') {
            if registry.contains(prov) {
                return Some(prov.to_string());
            }
        }
    }

    // 2. 按优先序从已注册 provider 中选择
    const PRIORITY: &[&str] = &[
        "anthropic", "openai-responses", "openai", "gemini", "deepseek", "groq",
        "mistral", "together", "openrouter", "xai", "moonshot",
    ];
    for &id in PRIORITY {
        if registry.contains(id) {
            return Some(id.to_string());
        }
    }

    // 3. 任意已注册的 provider
    let all = registry.list();
    if !all.is_empty() {
        return Some(all[0].id().to_string());
    }

    // 4. claude-code 兜底
    if registry.contains("claude-code") {
        return Some("claude-code".to_string());
    }

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
    register_one(RegisterParams {
        registry: &mut registry,
        client: &client,
        id,
        kind,
        api_key,
        base_url,
        default_model,
        extra_body: None,
    });
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
