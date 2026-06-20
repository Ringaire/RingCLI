//! Provider catalog：从 JSON 文件加载提供商定义，支持三层合并。
//!
//! 优先级（高 → 低）：项目级 `.neko/providers.json` > 全局 `~/.config/neko/providers.json` > 内置默认
//!
//! # 用户覆盖格式
//!
//! 全量新建提供商（需要 `type` + `base_url`）：
//! ```json
//! {
//!   "my-llm": { "name": "My LLM", "type": "openai-compatible", "base_url": "http://llm.internal/v1" }
//! }
//! ```
//!
//! 覆盖已知提供商的某个字段：
//! ```json
//! {
//!   "anthropic": { "base_url": "https://proxy.example.com/anthropic" }
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

// 内置默认目录（编译时嵌入）
const DEFAULT_JSON: &str = include_str!("providers.json");

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
    /// API 基础 URL（None 表示用 SDK 默认值）。
    pub base_url: Option<String>,
    /// 存放 API Key 的环境变量名；None = 不需要 key（如 Ollama）。
    pub api_key_env: Option<String>,
    /// 该 provider 的默认模型 id；None = 未知（如用户自定义 provider，需在 config.model 指定）。
    #[serde(default)]
    pub default_model: Option<String>,
    /// 注入到每个请求 body 顶层的额外字段（如 Ollama 的 `options.num_ctx`）。
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

/// 加载并合并 provider 目录。
///
/// - `global_config_dir`：`~/.config/neko/`（可 None）
/// - `project_dir`：项目根目录（可 None；函数内部会拼 `.neko/providers.json`）
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

/// 只返回内置默认目录（测试 / 无配置目录场景）。
pub fn defaults() -> HashMap<String, CatalogEntry> {
    serde_json::from_str(DEFAULT_JSON).expect("bundled providers.json is valid JSON")
}

/// 按 provider id 查找，不区分大小写。
pub fn get<'a>(catalog: &'a HashMap<String, CatalogEntry>, id: &str) -> Option<&'a CatalogEntry> {
    catalog.get(id).or_else(|| catalog.get(&id.to_lowercase()))
}

/// 返回某 provider 的默认模型 id（来自 catalog）；未知 provider / 未声明默认模型时返回 None。
pub fn default_model_for(catalog: &HashMap<String, CatalogEntry>, id: &str) -> Option<String> {
    get(catalog, id).and_then(|e| e.default_model.clone())
}

// ── 内部 ──────────────────────────────────────────────────────────────────────

fn merge_file(catalog: &mut HashMap<String, CatalogEntry>, path: &Path) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return, // 文件不存在或不可读，静默跳过
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
            // 覆盖已知 provider 的单个字段
            if let Some(n) = ov.name     { entry.name = n; }
            if let Some(k) = ov.kind     { entry.kind = k; }
            if let Some(u) = ov.base_url { entry.base_url = Some(u); }
            if let Some(e) = ov.api_key_env { entry.api_key_env = Some(e); }
            if let Some(m) = ov.default_model { entry.default_model = Some(m); }
            if let Some(b) = ov.extra_body { entry.extra_body = Some(b); }
        } else {
            // 新增自定义 provider：必须有 type + base_url
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
    fn merge_override_partial() {
        let mut cat = defaults();
        let ov: HashMap<String, Override> = serde_json::from_str(
            r#"{"anthropic": {"base_url": "https://proxy.example.com"}}"#,
        ).unwrap();
        for (id, o) in ov { if let Some(e) = cat.get_mut(&id) { if let Some(u) = o.base_url { e.base_url = Some(u); } } }
        assert_eq!(cat["anthropic"].base_url.as_deref(), Some("https://proxy.example.com"));
        // name 不变
        assert_eq!(cat["anthropic"].name, "Anthropic");
    }

    #[test]
    fn all_well_known_providers_present() {
        let cat = defaults();
        for id in &["deepseek", "groq", "mistral", "siliconflow", "zhipu", "ollama", "lmstudio"] {
            assert!(cat.contains_key(*id), "missing provider: {id}");
        }
    }
}
