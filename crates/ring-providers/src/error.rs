use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },

    #[error("JSON decode error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Rate limit exceeded (retry-after: {retry_after_secs:?}s)")]
    RateLimit { retry_after_secs: Option<u64> },

    #[error("Context length exceeded (max: {max}, got: {got})")]
    ContextLength { max: u64, got: u64 },

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Provider not configured: {0}")]
    NotConfigured(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}
