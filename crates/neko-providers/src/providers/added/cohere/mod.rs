//! Cohere provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Cohere".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.cohere.com/compatibility/v1".into()),
        api_key_env: Some("COHERE_API_KEY".into()),
        default_model: Some("command-r-plus".into()),
        extra_body: None,
    }
}
