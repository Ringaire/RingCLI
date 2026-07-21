//! Xiaomi（MiMo）provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "Xiaomi".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.miaomi.ai/v1".into()),
        api_key_env: Some("XIAOMI_API_KEY".into()),
        default_model: Some("MiMo-7B-RL".into()),
        extra_body: None,
    }
}
