//! Provider catalog：Rust 代码定义内置 provider，支持用户 JSON 覆盖。
//!
//! 内置 provider 定义在各 provider 子模块的 `catalog_entry()` 函数中。
//! 优先级（高 → 低）：项目级 `.neko/providers.json` > 全局 `~/.config/neko/providers.json` > 内置默认

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ── 类型 ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

/// 完整的 provider 定义（合并后的最终形态）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub extra_body: Option<serde_json::Value>,
}

/// 用户覆盖文件里的条目——所有字段都是可选的。
#[derive(Debug, Default, Deserialize)]
struct Override {
    name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<ProviderKind>,
    base_url: Option<String>,
    api_key_env: Option<String>,
    default_model: Option<String>,
    extra_body: Option<serde_json::Value>,
}

// ── 公开 API ──────────────────────────────────────────────────────────────────

pub fn load(
    global_config_dir: Option<&Path>,
    project_dir: Option<&Path>,
) -> HashMap<String, CatalogEntry> {
    let mut catalog = defaults();

    if let Some(dir) = global_config_dir {
        merge_file(&mut catalog, &dir.join("providers.json"));
    }
    if let Some(dir) = project_dir {
        merge_file(&mut catalog, &dir.join(".neko").join("providers.json"));
    }

    catalog
}

/// 内置默认目录——从各 provider 子模块的 `catalog_entry()` 构建。
pub fn defaults() -> HashMap<String, CatalogEntry> {
    use crate::providers;

    let mut m = HashMap::new();

    // 原生 provider
    m.insert("anthropic".into(), providers::anthropic::catalog_entry());
    m.insert("openai".into(), providers::openai::catalog_entry());
    m.insert("gemini".into(), providers::gemini::catalog_entry());

    // 兼容 provider
    use crate::providers::added;
    m.insert("deepseek".into(),    added::deepseek::catalog_entry());
    m.insert("groq".into(),        added::groq::catalog_entry());
    m.insert("mistral".into(),     added::mistral::catalog_entry());
    m.insert("together".into(),    added::together::catalog_entry());
    m.insert("openrouter".into(),  added::openrouter::catalog_entry());
    m.insert("xai".into(),         added::xai::catalog_entry());
    m.insert("moonshot".into(),    added::moonshot::catalog_entry());
    m.insert("siliconflow".into(), added::siliconflow::catalog_entry());
    m.insert("zhipu".into(),       added::zhipu::catalog_entry());
    m.insert("baidu".into(),       added::baidu::catalog_entry());
    m.insert("cerebras".into(),    added::cerebras::catalog_entry());
    m.insert("deepinfra".into(),   added::deepinfra::catalog_entry());
    m.insert("fireworks".into(),   added::fireworks::catalog_entry());
    m.insert("perplexity".into(),  added::perplexity::catalog_entry());
    m.insert("cohere".into(),      added::cohere::catalog_entry());
    m.insert("nvidia".into(),      added::nvidia::catalog_entry());

    // 本地 provider
    m.insert("ollama".into(),      added::ollama::catalog_entry());
    m.insert("lmstudio".into(),    added::lmstudio::catalog_entry());

    m
}

pub fn get<'a>(catalog: &'a HashMap<String, CatalogEntry>, id: &str) -> Option<&'a CatalogEntry> {
    catalog.get(id).or_else(|| catalog.get(&id.to_lowercase()))
}

pub fn default_model_for(catalog: &HashMap<String, CatalogEntry>, id: &str) -> Option<String> {
    get(catalog, id).and_then(|e| e.default_model.clone())
}

// ── 内部 ──────────────────────────────────────────────────────────────────────

fn merge_file(catalog: &mut HashMap<String, CatalogEntry>, path: &Path) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return,
    };
    let overrides: HashMap<String, Override> = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(path = %path.display(), err = %e, "failed to parse providers.json override");
            return;
        }
    };
    for (id, ov) in overrides {
        if let Some(entry) = catalog.get_mut(&id) {
            if let Some(n) = ov.name     { entry.name = n; }
            if let Some(k) = ov.kind     { entry.kind = k; }
            if let Some(u) = ov.base_url { entry.base_url = Some(u); }
            if let Some(e) = ov.api_key_env { entry.api_key_env = Some(e); }
            if let Some(m) = ov.default_model { entry.default_model = Some(m); }
            if let Some(b) = ov.extra_body { entry.extra_body = Some(b); }
        } else {
            let Some(kind) = ov.kind else {
                tracing::warn!(id, "custom provider missing 'type', skipping");
                continue;
            };
            let Some(base_url) = ov.base_url else {
                tracing::warn!(id, "custom provider missing 'base_url', skipping");
                continue;
            };
            catalog.insert(id.clone(), CatalogEntry {
                name: ov.name.unwrap_or_else(|| id.clone()),
                kind,
                base_url: Some(base_url),
                api_key_env: ov.api_key_env,
                default_model: ov.default_model,
                extra_body: ov.extra_body,
            });
        }
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let cat = defaults();
        assert!(cat.contains_key("anthropic"));
        assert!(cat.contains_key("openai"));
        assert!(cat.contains_key("gemini"));
        assert!(cat.contains_key("ollama"));
        assert_eq!(cat["anthropic"].kind, ProviderKind::Anthropic);
        assert_eq!(cat["deepseek"].kind, ProviderKind::OpenAiCompatible);
    }

    #[test]
    fn all_well_known_providers_present() {
        let cat = defaults();
        for id in &[
            "deepseek", "groq", "mistral", "siliconflow", "zhipu",
            "ollama", "lmstudio", "together", "openrouter", "xai",
            "moonshot", "baidu", "cerebras", "deepinfra", "fireworks",
            "perplexity", "cohere", "nvidia",
        ] {
            assert!(cat.contains_key(*id), "missing provider: {id}");
        }
    }
}
