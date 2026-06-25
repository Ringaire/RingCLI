//! Baidu ERNIE provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Baidu ERNIE".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://qianfan.baidubce.com/v2".into()),
        api_key_env: Some("BAIDU_API_KEY".into()),
        default_model: Some("ernie-4.0-turbo-8k".into()),
        extra_body: None,
    }
}
