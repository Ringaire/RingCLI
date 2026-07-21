//! Xiaomi AMS（阿姆斯特丹区域）provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "Xiaomi AMS".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api-ams.miaomi.ai/v1".into()),
        api_key_env: Some("XIAOMI_API_KEY".into()),
        default_model: Some("MiMo-7B-RL".into()),
        extra_body: None,
    }
}
