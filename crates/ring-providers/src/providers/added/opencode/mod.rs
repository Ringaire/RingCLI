//! OpenCode provider 配置（开源 Agent 的 API 端点）。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "OpenCode".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("http://localhost:3000/v1".into()),
        api_key_env: None,
        default_model: None,
        extra_body: None,
    }
}
