//! Cloudflare AI Gateway（代理模式）provider 配置。
//! 需要用户在 base_url 中填写完整的 gateway URL（含 account / gateway / model slug）。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "Cloudflare AI Gateway".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://gateway.ai.cloudflare.com/v1".into()),
        api_key_env: Some("CLOUDFLARE_API_TOKEN".into()),
        default_model: None,
        extra_body: None,
    }
}
