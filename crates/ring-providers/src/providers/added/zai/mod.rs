//! ZAI Coding provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "ZAI Coding".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.z-ai.io/v1".into()),
        api_key_env: Some("ZAI_API_KEY".into()),
        default_model: Some("z-ai-coder-v1".into()),
        extra_body: None,
    }
}
