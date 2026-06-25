//! Fireworks provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Fireworks".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.fireworks.ai/inference/v1".into()),
        api_key_env: Some("FIREWORKS_API_KEY".into()),
        default_model: Some("accounts/fireworks/models/llama-v3p1-70b-instruct".into()),
        extra_body: None,
    }
}
