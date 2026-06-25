//! Groq provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Groq".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.groq.com/openai/v1".into()),
        api_key_env: Some("GROQ_API_KEY".into()),
        default_model: Some("llama-3.3-70b-versatile".into()),
        extra_body: None,
    }
}
