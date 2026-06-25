//! DeepSeek provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "DeepSeek".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.deepseek.com/v1".into()),
        api_key_env: Some("DEEPSEEK_API_KEY".into()),
        default_model: Some("deepseek-chat".into()),
        extra_body: None,
    }
}
