//! Perplexity provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Perplexity".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.perplexity.ai/v1".into()),
        api_key_env: Some("PERPLEXITY_API_KEY".into()),
        default_model: Some("sonar".into()),
        extra_body: None,
    }
}
