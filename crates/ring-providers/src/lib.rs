pub mod catalog;
pub mod error;
pub mod factory;
pub mod provider;
pub mod providers;
pub mod registry;

pub use catalog::{default_model_for, CatalogEntry, ProviderKind};
pub use error::ProviderError;
pub use factory::{
    build_probe_provider, build_registry, known_provider_ids, split_model_ref, ProviderBootstrap,
};
pub use provider::{
    build_http_client, ChatRequest, ChatResponse, ModelInfo, Provider, StreamChunk,
    StreamEvent, ToolDef, Usage,
};
pub use registry::ProviderRegistry;
