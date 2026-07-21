//! MiniMax provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "MiniMax".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.minimax.chat/v1".into()),
        api_key_env: Some("MINIMAX_API_KEY".into()),
        default_model: Some("MiniMax-Text-01".into()),
        extra_body: None,
    }
}
