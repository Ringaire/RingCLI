//! Ant Ling（蚂蚁灵码）provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "Ant Ling".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://ling-api.antgroup.com/v1".into()),
        api_key_env: Some("ANT_LING_API_KEY".into()),
        default_model: Some("ant-ling-v1".into()),
        extra_body: None,
    }
}
