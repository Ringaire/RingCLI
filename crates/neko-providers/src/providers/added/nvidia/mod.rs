//! NVIDIA provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "NVIDIA".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://integrate.api.nvidia.com/v1".into()),
        api_key_env: Some("NVIDIA_API_KEY".into()),
        default_model: Some("meta/llama-3.1-70b-instruct".into()),
        extra_body: None,
    }
}
