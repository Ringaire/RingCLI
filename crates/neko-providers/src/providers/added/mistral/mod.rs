//! Mistral provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Mistral".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.mistral.ai/v1".into()),
        api_key_env: Some("MISTRAL_API_KEY".into()),
        default_model: Some("mistral-large-latest".into()),
        extra_body: None,
    }
}
