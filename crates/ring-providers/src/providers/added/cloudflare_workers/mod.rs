//! Cloudflare Workers AI provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "Cloudflare Workers AI".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/v1".into()),
        api_key_env: Some("CLOUDFLARE_API_TOKEN".into()),
        default_model: Some("@cf/meta/llama-3.1-70b-instruct".into()),
        extra_body: None,
    }
}
