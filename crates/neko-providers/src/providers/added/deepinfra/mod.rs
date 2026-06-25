//! DeepInfra provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

/// 该 provider 的 catalog 条目。
pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        name: "DeepInfra".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api.deepinfra.com/v1/openai".into()),
        api_key_env: Some("DEEPINFRA_API_KEY".into()),
        default_model: Some("meta-llama/Meta-Llama-3.1-70B-Instruct".into()),
        extra_body: None,
    }
}
