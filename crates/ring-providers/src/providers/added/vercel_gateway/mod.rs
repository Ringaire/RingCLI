//! Vercel AI Gateway（代理模式）provider 配置。
//! 代理模式下 base_url 需要用户通过 providers.json 覆盖。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "Vercel AI Gateway".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.vercel.ai/v1".into()),
        api_key_env: None,
        default_model: None,
        extra_body: None,
    }
}
