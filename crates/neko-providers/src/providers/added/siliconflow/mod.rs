//! SiliconFlow provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "SiliconFlow".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.siliconflow.cn/v1".into()),
        api_key_env: Some("SILICONFLOW_API_KEY".into()),
        default_model: Some("Qwen/Qwen2.5-72B-Instruct".into()),
        extra_body: None,
    }
}
