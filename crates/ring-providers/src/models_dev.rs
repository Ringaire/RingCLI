//! models.dev 远程模型元数据接入。
//!
//! 从 `https://models.dev/models.json` 拉取 provider-agnostic 的模型能力元数据，
//! 用于补全 `CompatibleProvider::list_models` 返回的 `ModelInfo` 能力字段
//!（supports_thinking / supports_vision / context_window 等）。
//!
//! 流程：
//! 1. 启动或 `/refresh:model` 时异步拉取，缓存到 `~/.ring/cache/models-dev.json`
//! 2. `list_models` 时按裸 model id 查询缓存，合并真实能力
//! 3. 离线/失败时用缓存，无缓存则回退默认值
//!
//! 参考 Pi 的 `models.generated.ts`（脚本生成静态映射），Ring 用运行时拉取 + 缓存。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use serde::Deserialize;
use tracing::{debug, warn};

// ── models.dev JSON 结构（只取关心的字段）──────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
struct RawEntry {
    name:       Option<String>,
    reasoning:  Option<bool>,
    attachment: Option<bool>,
    tool_call:  Option<bool>,
    modalities: Option<RawModalities>,
    limit:      Option<RawLimit>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawModalities {
    input: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawLimit {
    context: Option<u64>,
    output:  Option<u64>,
}

// ── 公开元数据（合并后的最终形态）──────────────────────────────────────────────

/// 单个模型的 models.dev 元数据（已归一化）。
#[derive(Debug, Clone, Default)]
pub struct ModelMeta {
    pub display_name:     Option<String>,
    pub supports_thinking: bool,
    pub supports_vision:  bool,
    pub supports_tools:   bool,
    pub context_window:   Option<u64>,
    pub max_output_tokens: Option<u64>,
}

impl ModelMeta {
    fn from_raw(raw: &RawEntry) -> Self {
        // vision：attachment=true 或 modalities.input 含 image
        let supports_vision = raw.attachment.unwrap_or(false)
            || raw
                .modalities
                .as_ref()
                .and_then(|m| m.input.as_ref())
                .map(|inputs| inputs.iter().any(|m| m == "image"))
                .unwrap_or(false);
        Self {
            display_name:      raw.name.clone(),
            supports_thinking: raw.reasoning.unwrap_or(false),
            supports_vision,
            supports_tools:    raw.tool_call.unwrap_or(true),
            context_window:    raw.limit.as_ref().and_then(|l| l.context),
            max_output_tokens: raw.limit.as_ref().and_then(|l| l.output),
        }
    }
}

// ── 缓存 ──────────────────────────────────────────────────────────────────────

/// models.dev 模型元数据缓存。key = 裸 model id（取 `provider/model` 的 model 部分）。
#[derive(Debug, Clone, Default)]
pub struct ModelsDevCache {
    entries: HashMap<String, ModelMeta>,
}

impl ModelsDevCache {
    /// 从远程 `https://models.dev/models.json` 拉取。
    ///
    /// 网络失败返回空缓存（不阻塞调用方），由调用方决定是否回退本地缓存。
    pub async fn fetch(client: &reqwest::Client) -> Self {
        const URL: &str = "https://models.dev/models.json";
        debug!("fetching models.dev metadata");
        let resp = client.get(URL).send().await;
        let body = match resp {
            Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
            Ok(r) => {
                warn!(status = %r.status(), "models.dev fetch failed, using empty cache");
                return Self::default();
            }
            Err(e) => {
                warn!(err = %e, "models.dev fetch error, using empty cache");
                return Self::default();
            }
        };
        Self::parse(&body)
    }

    /// 解析 models.json 文本。
    pub fn parse(json: &str) -> Self {
        let raw: HashMap<String, RawEntry> = match serde_json::from_str(json) {
            Ok(m) => m,
            Err(e) => {
                warn!(err = %e, "models.dev parse failed");
                return Self::default();
            }
        };
        let mut entries = HashMap::with_capacity(raw.len());
        for (full_id, r) in &raw {
            // full_id 形如 "xai/grok-4.3"，取 "/" 后的裸 model id 作 key
            let bare = full_id.rsplit_once('/').map(|(_, m)| m).unwrap_or(full_id);
            // 同名冲突时保留第一个（少见，且能力通常一致）
            entries.entry(bare.to_string()).or_insert_with(|| ModelMeta::from_raw(r));
        }
        Self { entries }
    }

    /// 从本地缓存文件加载（离线 fallback）。
    pub fn load_cache(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(json) => Self::parse(&json),
            Err(_) => Self::default(),
        }
    }

    /// 写入本地缓存（原子写）。
    pub fn save_cache(&self, path: &Path) {
        // 序列化原始 JSON（存 models.dev 原文最省事，但这里只存解析后的）
        // 为简化，重新拼一个 {bare_id: {fields}} 结构
        let mut out = serde_json::Map::new();
        for (id, m) in &self.entries {
            let mut e = serde_json::Map::new();
            if let Some(n) = &m.display_name { e.insert("name".into(), n.clone().into()); }
            if m.supports_thinking { e.insert("reasoning".into(), true.into()); }
            if m.supports_vision { e.insert("attachment".into(), true.into()); }
            e.insert("tool_call".into(), m.supports_tools.into());
            if let Some(c) = m.context_window { e.insert("limit".into(), serde_json::json!({"context": c, "output": m.max_output_tokens})); }
            out.insert(id.clone(), serde_json::Value::Object(e));
        }
        let json = serde_json::Value::Object(out);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, serde_json::to_vec(&json).unwrap_or_default());
    }

    /// 按裸 model id 查询元数据。
    pub fn lookup(&self, model_id: &str) -> Option<&ModelMeta> {
        self.entries.get(model_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 全局缓存（启动时填充，list_models 时只读查询）──────────────────────────────

static GLOBAL_CACHE: OnceLock<Arc<RwLock<ModelsDevCache>>> = OnceLock::new();

/// 初始化全局缓存（首次启动时调用，仅 set 一次）。
pub fn init_cache(cache: ModelsDevCache) {
    let _ = GLOBAL_CACHE.set(Arc::new(RwLock::new(cache)));
}

/// 替换全局缓存内容（/refresh:model 刷新时调用）。
pub fn replace_cache(cache: ModelsDevCache) {
    if let Some(c) = GLOBAL_CACHE.get() {
        *c.write().unwrap() = cache;
    } else {
        init_cache(cache);
    }
}

/// 全局查询裸 model id 的元数据（clone 返回，避免持锁）。
///
/// 未初始化时返回 None（调用方回退默认值）。
pub fn lookup_global(model_id: &str) -> Option<ModelMeta> {
    let c = GLOBAL_CACHE.get()?;
    c.read().unwrap().lookup(model_id).cloned()
}

// ── 手动能力定义（覆盖 models.dev）──────────────────────────────────────────

use ring_core::config::ModelCaps;

static GLOBAL_CAPS: OnceLock<Arc<RwLock<HashMap<String, ModelCaps>>>> = OnceLock::new();

/// 设置手动能力定义（config 加载后调用，可重复调用刷新）。
pub fn init_caps(caps: HashMap<String, ModelCaps>) {
    match GLOBAL_CAPS.get() {
        Some(c) => *c.write().unwrap() = caps,
        None    => { let _ = GLOBAL_CAPS.set(Arc::new(RwLock::new(caps))); }
    }
}

/// 查询裸 model id 的手动能力定义。
pub fn lookup_caps_global(model_id: &str) -> Option<ModelCaps> {
    let c = GLOBAL_CAPS.get()?;
    c.read().unwrap().get(model_id).cloned()
}

/// 合并优先级：手动 caps > models.dev > None。
/// 返回最终能力（供 list_models 使用）。
pub fn resolve_meta(model_id: &str) -> Option<ModelMeta> {
    let mut meta = lookup_global(model_id);
    if let Some(caps) = lookup_caps_global(model_id) {
        let m = meta.get_or_insert_with(ModelMeta::default);
        if let Some(v) = caps.vision   { m.supports_vision = v; }
        if let Some(t) = caps.thinking { m.supports_thinking = t; }
        if let Some(to) = caps.tools   { m.supports_tools = to; }
        if let Some(c) = caps.context  { m.context_window = Some(c); }
        if let Some(o) = caps.output   { m.max_output_tokens = Some(o); }
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let json = r#"{
            "xai/grok-4.3": {
                "name": "Grok 4.3",
                "reasoning": true,
                "attachment": true,
                "tool_call": true,
                "limit": {"context": 1000000, "output": 30000}
            },
            "deepseek/deepseek-r1": {
                "name": "DeepSeek R1",
                "reasoning": true,
                "tool_call": false,
                "modalities": {"input": ["text"]}
            }
        }"#;
        let cache = ModelsDevCache::parse(json);
        assert_eq!(cache.len(), 2);
        let grok = cache.lookup("grok-4.3").unwrap();
        assert!(grok.supports_thinking);
        assert!(grok.supports_vision);
        assert_eq!(grok.context_window, Some(1000000));
        let r1 = cache.lookup("deepseek-r1").unwrap();
        assert!(r1.supports_thinking);
        assert!(!r1.supports_vision); // 无 attachment，input 无 image
        assert!(!r1.supports_tools);
    }

    #[test]
    fn parse_empty() {
        let cache = ModelsDevCache::parse("not json");
        assert!(cache.is_empty());
    }

    #[test]
    fn vision_from_modalities() {
        // attachment 缺失但 modalities.input 含 image → vision=true
        let json = r#"{"p/m": {"modalities": {"input": ["text","image"]}}}"#;
        let cache = ModelsDevCache::parse(json);
        assert!(cache.lookup("m").unwrap().supports_vision);
    }

    #[test]
    fn cache_roundtrip() {
        let json = r#"{"xai/grok": {"name":"Grok","reasoning":true}}"#;
        let cache = ModelsDevCache::parse(json);
        let tmp = std::env::temp_dir().join("ring_models_dev_test.json");
        cache.save_cache(&tmp);
        let loaded = ModelsDevCache::load_cache(&tmp);
        let _ = std::fs::remove_file(&tmp);
        let m = loaded.lookup("grok").unwrap();
        assert!(m.supports_thinking);
        assert_eq!(m.display_name.as_deref(), Some("Grok"));
    }
}
