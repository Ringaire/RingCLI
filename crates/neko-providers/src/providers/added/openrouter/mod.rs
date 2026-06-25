//! OpenRouter provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "OpenRouter".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://openrouter.ai/api/v1".into()),
        api_key_env: Some("OPENROUTER_API_KEY".into()),
        default_model: Some("anthropic/claude-sonnet-4-6".into()),
        extra_body: None,
    }
}
