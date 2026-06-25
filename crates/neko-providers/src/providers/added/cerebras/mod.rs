//! Cerebras provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Cerebras".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.cerebras.ai/v1".into()),
        api_key_env: Some("CEREBRAS_API_KEY".into()),
        default_model: Some("llama-3.3-70b".into()),
        extra_body: None,
    }
}
