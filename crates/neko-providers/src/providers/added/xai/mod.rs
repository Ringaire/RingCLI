//! xAI provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "xAI".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.x.ai/v1".into()),
        api_key_env: Some("XAI_API_KEY".into()),
        default_model: Some("grok-2-latest".into()),
        extra_body: None,
    }
}
