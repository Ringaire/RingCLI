use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Transport closed")]
    Closed,

    #[error("RPC error {code}: {message}")]
    Rpc { code: i64, message: String },

    #[error("Timeout")]
    Timeout,

    #[error("{0}")]
    Other(String),
}
