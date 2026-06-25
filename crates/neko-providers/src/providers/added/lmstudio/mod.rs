//! LM Studio provider 配置（本地，无需 API key）。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "LM Studio".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("http://localhost:1234/v1".into()),
        api_key_env: None,
        default_model: Some("local-model".into()),
        extra_body: Some(serde_json::json!({"options": {"num_ctx": 32768}})),
    }
}
