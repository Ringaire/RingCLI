//! Zhipu AI provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "Zhipu AI".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()),
        api_key_env: Some("ZHIPU_API_KEY".into()),
        default_model: Some("glm-4".into()),
        extra_body: None,
    }
}
