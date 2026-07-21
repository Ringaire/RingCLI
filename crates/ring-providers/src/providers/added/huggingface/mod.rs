//! HuggingFace Inference API provider 配置。

use crate::catalog::{CatalogEntry, ProviderKind};

pub fn catalog_entry() -> CatalogEntry {
    CatalogEntry {
        api_key: None,
        name: "HuggingFace".into(),
        kind: ProviderKind::OpenAiCompatible,
        base_url: Some("https://api-inference.huggingface.co/v1".into()),
        api_key_env: Some("HF_TOKEN".into()),
        default_model: Some("meta-llama/Meta-Llama-3.1-70B-Instruct".into()),
        extra_body: None,
    }
}
