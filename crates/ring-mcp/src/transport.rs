use async_trait::async_trait;

use crate::error::McpError;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&mut self, req: JsonRpcRequest) -> Result<(), McpError>;
    async fn recv(&mut self) -> Result<JsonRpcResponse, McpError>;
}

// ── Stdio transport ────────────────────────────────────────────────────────────

pub struct StdioTransport {
    stdin:  tokio::process::ChildStdin,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
    _child: tokio::process::Child,
}

impl StdioTransport {
    pub async fn spawn(command: &str, args: &[String], env: &[(String, String)]) -> Result<Self, McpError> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn()?;
        let stdin  = child.stdin.take().ok_or_else(|| McpError::Other("no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Other("no stdout".into()))?;
        Ok(Self { stdin, stdout: tokio::io::BufReader::new(stdout), _child: child })
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send(&mut self, req: JsonRpcRequest) -> Result<(), McpError> {
        use tokio::io::AsyncWriteExt;
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<JsonRpcResponse, McpError> {
        use tokio::io::AsyncBufReadExt;
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).await?;
        if n == 0 { return Err(McpError::Closed); }
        let resp = serde_json::from_str(line.trim())?;
        Ok(resp)
    }
}

// ── SSE transport ──────────────────────────────────────────────────────────────

pub struct SseTransport {
    client:   reqwest::Client,
    base_url: String,
    headers:  std::collections::HashMap<String, String>,
}

impl SseTransport {
    pub fn new(url: impl Into<String>, headers: std::collections::HashMap<String, String>) -> Result<Self, McpError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| McpError::Other(format!("failed to build HTTP client - check TLS configuration: {}", e)))?;
        Ok(Self { client, base_url: url.into(), headers })
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn send(&mut self, req: JsonRpcRequest) -> Result<(), McpError> {
        let mut builder = self.client.post(&self.base_url).json(&req);
        for (k, v) in &self.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        builder.send().await.map_err(|e| McpError::Other(e.to_string()))?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<JsonRpcResponse, McpError> {
        Err(McpError::Other("SSE streaming recv not implemented in this direction".into()))
    }
}
