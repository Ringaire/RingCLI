//! Qwen（阿里通义千问）Token Plan provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "Qwen".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1".into()),
        api_key_env: Some("DASHSCOPE_API_KEY".into()),
        default_model: Some("qwen-plus".into()),
        extra_body: None,
    }
}
