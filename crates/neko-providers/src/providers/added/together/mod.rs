//! Together AI provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Together AI".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.together.xyz/v1".into()),
        api_key_env: Some("TOGETHER_API_KEY".into()),
        default_model: Some("meta-llama/Llama-3-70b-chat-hf".into()),
        extra_body: None,
    }
}
