 // OpenAI-compatible provider（DeepSeek, Groq, Mistral, Together, Ollama 等）
// Provider 定义现在来自 catalog（providers.json），此处只保留运行时实现。

use async_trait::async_trait;
use reqwest::Client;
use tokio_util::sync::CancellationToken;

use crate::error::ProviderError;
use crate::provider::{ChatRequest, ChatResponse, ModelInfo, Provider, ProviderStream};
use crate::providers::openai::OpenAiProvider;

pub struct CompatibleProvider {
    inner:     OpenAiProvider,
    id:        String,
    name:      String,
    def_model: String,
}

impl CompatibleProvider {
    pub fn new(
        id:        impl Into<String>,
        name:      impl Into<String>,
        api_key:   impl Into<String>,
        base_url:  impl Into<String>,
        def_model: impl Into<String>,
    ) -> Self {
        let client = crate::provider::build_http_client(None, crate::provider::DEFAULT_CONNECT_TIMEOUT_SECS);
        Self::with_client(client, id, name, api_key, base_url, def_model)
    }

    pub fn with_client(
        client:    Client,
        id:        impl Into<String>,
        name:      impl Into<String>,
        api_key:   impl Into<String>,
        base_url:  impl Into<String>,
        def_model: impl Into<String>,
    ) -> Self {
        Self::with_client_and_extra(client, id, name, api_key, base_url, def_model, None)
    }

    pub fn with_client_and_extra(
        client:     Client,
        id:         impl Into<String>,
        name:       impl Into<String>,
        api_key:    impl Into<String>,
        base_url:   impl Into<String>,
        def_model:  impl Into<String>,
        extra_body: Option<serde_json::Value>,
    ) -> Self {
        let id_str   = id.into();
        let name_str = name.into();
        let def      = def_model.into();
        let mut inner = OpenAiProvider::with_client(
            client,
            api_key,
            Some(base_url.into()),
            None,
            Some(def.clone()),
        );
        if let Some(extra) = extra_body {
            inner = inner.with_extra_body(extra);
        }
        Self { inner, id: id_str, name: name_str, def_model: def }
    }
}

#[async_trait]
impl Provider for CompatibleProvider {
    fn id(&self) -> &str { &self.id }
    fn display_name(&self) -> &str { &self.name }
    fn default_model(&self) -> &str { &self.def_model }

    async fn chat(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ChatResponse, ProviderError> {
        self.inner.chat(req, signal).await
    }

    async fn stream(&self, req: &ChatRequest, signal: CancellationToken) -> Result<ProviderStream, ProviderError> {
        self.inner.stream(req, signal).await
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
}
