//! OpenAI/ChatGPT OAuth2 认证（Authorization Code + PKCE）。
//!
//! 复用 Codex CLI 的公共 OAuth 应用（PKCE 流程不需要 client_secret，
//! client_id 公开是安全的）。
//!
//! 流程：
//! 1. 生成 PKCE pair（verifier + challenge）
//! 2. 启动本地回调服务器（127.0.0.1:1455）
//! 3. 打开浏览器到 auth.openai.com/oauth/authorize
//! 4. 用户登录授权 → 回调带回 authorization code
//! 5. 用 code + verifier 换 token（POST /oauth/token）
//! 6. 用 id_token 通过 Token Exchange 换取 API key
//! 7. 持久化到 ~/.config/neko/auth.json（权限 0600）
//!
//! 对照：Codex `codex-rs/login/`、OpenCode `openai.ts`

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

// ── 常量 ──────────────────────────────────────────────────────────────────────

/// Codex CLI 的公共 OAuth 应用 ID（PKCE 不需要 secret，公开安全）。
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// OAuth 发行方。
pub const ISSUER: &str = "https://auth.openai.com";

/// 本地回调服务器端口。
pub const CALLBACK_PORT: u16 = 1455;

/// 回调 URL。
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";

/// Token 请求超时。
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);

// ── PKCE ──────────────────────────────────────────────────────────────────────

/// PKCE (Proof Key for Code Exchange) pair.
#[derive(Clone)]
pub struct Pkce {
    pub verifier:  String,
    pub challenge: String,
}

impl Pkce {
    /// 生成新的 PKCE pair：64 字节随机 → base64url verifier → SHA256 → base64url challenge。
    pub fn generate() -> Self {
        let mut bytes = [0u8; 64];
        rand::thread_rng().fill_bytes(&mut bytes);
        let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let challenge = {
            let mut hasher = Sha256::new();
            hasher.update(verifier.as_bytes());
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
        };
        Self { verifier, challenge }
    }
}

/// 生成 32 字节随机 state（CSRF 保护）。
fn random_state() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

// ── Token 数据结构 ────────────────────────────────────────────────────────────

/// OAuth2 token 响应（来自 /oauth/token）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token:  String,
    pub refresh_token: String,
    pub id_token:      String,
    pub expires_in:    Option<u64>,
    pub token_type:    Option<String>,
}

/// Token Exchange 响应（RFC 8693，用 id_token 换 API key）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyExchangeResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
}

/// 持久化的认证数据（~/.config/neko/auth.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthData {
    pub auth_mode:      String,
    pub openai_api_key: Option<String>,
    pub tokens:         Option<StoredTokens>,
    pub last_refresh:   Option<String>,
}

/// 持久化的 token 子结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokens {
    pub access_token:  String,
    pub refresh_token: String,
    pub id_token:      Option<String>,
    pub account_id:    Option<String>,
}

impl AuthData {
    pub fn chatgpt(api_key: Option<String>, tokens: TokenResponse) -> Self {
        let account_id = extract_account_id(&tokens.id_token)
            .or_else(|| extract_account_id(&tokens.access_token));
        Self {
            auth_mode:      "chatgpt".into(),
            openai_api_key: api_key,
            tokens:         Some(StoredTokens {
                access_token:  tokens.access_token,
                refresh_token: tokens.refresh_token,
                id_token:      Some(tokens.id_token),
                account_id,
            }),
            last_refresh:   Some(chrono::Utc::now().to_rfc3339()),
        }
    }

    /// 获取 API key（优先用 token exchange 换来的，其次直接存的）。
    pub fn api_key(&self) -> Option<&str> {
        self.openai_api_key.as_deref()
    }

    /// 是否有 refresh_token（可用于自动刷新）。
    pub fn has_refresh(&self) -> bool {
        self.tokens.as_ref().map(|t| !t.refresh_token.is_empty()).unwrap_or(false)
    }
}

// ── JWT claim 提取 ────────────────────────────────────────────────────────────

/// 从 JWT 中提取 account_id（不验证签名，仅读 claim）。
fn extract_account_id(jwt: &str) -> Option<String> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(parts[1]))
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    // claim 路径：chatgpt_account_id 或 https://api.openai.com/auth.chatgpt_account_id
    value.get("chatgpt_account_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            value.get("https://api.openai.com/auth")
                .and_then(|a| a.get("chatgpt_account_id"))
                .and_then(|v| v.as_str())
        })
        .map(|s| s.to_string())
}

// ── OAuth2 流程 ───────────────────────────────────────────────────────────────

/// OAuth2 认证错误。
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("callback server error: {0}")]
    Server(String),
    #[error("browser open failed: {0}")]
    Browser(String),
    #[error("auth failed: {0}")]
    Auth(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// 构造浏览器授权 URL。
pub fn authorize_url(pkce: &Pkce, state: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", REDIRECT_URI),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", "neko"),
    ];
    let qs = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().map(|(k, v)| (*k, *v)))
        .finish();
    format!("{ISSUER}/oauth/authorize?{qs}")
}

/// 用系统默认浏览器打开 URL。
fn open_browser(url: &str) -> Result<(), OAuthError> {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", url]).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    result.map(|_| ()).map_err(|e| OAuthError::Browser(e.to_string()))
}

/// 浏览器 OAuth2 登录流程。
///
/// 1. 启动本地回调服务器
/// 2. 打开浏览器到授权页面
/// 3. 等待用户授权，回调带回 code
/// 4. 用 code + verifier 换 token
/// 5. 用 id_token 通过 Token Exchange 换取 API key
pub async fn login_browser(client: &Client) -> Result<AuthData, OAuthError> {
    let pkce = Pkce::generate();
    let state = random_state();

    // 构造授权 URL
    let url = authorize_url(&pkce, &state);
    info!(url = %url, "starting browser OAuth2 flow");

    // 启动本地回调服务器
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
        .await
        .map_err(|e| OAuthError::Server(format!("bind {CALLBACK_PORT} failed: {e}")))?;

    // 打开浏览器
    open_browser(&url).map_err(|e| {
        warn!(error = %e, "failed to open browser, user should visit URL manually");
        // 不返回错误——用户可以手动打开 URL
    });

    // 等待回调（5 分钟超时）
    let code = wait_for_callback(&listener, &state, Duration::from_secs(300)).await?;

    // 换 token
    let tokens = exchange_code_for_tokens(client, &code, &pkce.verifier).await?;
    info!("token exchange successful");

    // Token Exchange：用 id_token 换取 API key
    let api_key = obtain_api_key(client, &tokens.id_token).await.ok();
    if api_key.is_none() {
        warn!("token exchange for API key failed, proceeding with access_token only");
    }

    Ok(AuthData::chatgpt(api_key, tokens))
}

/// 设备码 OAuth2 登录流程（headless / SSH 环境）。
///
/// 1. 请求 device code + user_code
/// 2. 用户在浏览器中输入 user_code
/// 3. 轮询直到用户授权
/// 4. 换 token
pub async fn login_device(client: &Client) -> Result<AuthData, OAuthError> {
    // 请求 device auth
    let device_resp: serde_json::Value = client
        .post(format!("{ISSUER}/api/accounts/deviceauth/usercode"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "client_id": CLIENT_ID }))
        .send()
        .await?
        .json()
        .await?;

    let device_auth_id = device_resp["device_auth_id"]
        .as_str()
        .ok_or_else(|| OAuthError::Auth("missing device_auth_id".into()))?
        .to_string();
    let user_code = device_resp["user_code"]
        .as_str()
        .ok_or_else(|| OAuthError::Auth("missing user_code".into()))?
        .to_string();
    let interval: u64 = device_resp["interval"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let device_url = format!("{ISSUER}/codex/device");
    info!(user_code = %user_code, device_url = %device_url, "device code flow");

    // 尝试打开浏览器
    let _ = open_browser(&device_url);

    // 轮询
    let poll_interval = Duration::from_secs(interval.max(1));
    let max_attempts = 60; // 最多轮询 5 分钟
    for _ in 0..max_attempts {
        tokio::time::sleep(poll_interval).await;

        let resp = client
            .post(format!("{ISSUER}/api/accounts/deviceauth/token"))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "device_auth_id": &device_auth_id,
                "user_code": &user_code,
            }))
            .send()
            .await?;

        if resp.status().is_success() {
            let data: serde_json::Value = resp.json().await?;
            let auth_code = data["authorization_code"]
                .as_str()
                .ok_or_else(|| OAuthError::Auth("missing authorization_code".into()))?;
            let code_verifier = data["code_verifier"]
                .as_str()
                .ok_or_else(|| OAuthError::Auth("missing code_verifier".into()))?;

            // 服务端生成了 PKCE pair，直接用它
            let pkce = Pkce {
                verifier: code_verifier.to_string(),
                challenge: String::new(), // 不需要了
            };
            let tokens = exchange_code_for_tokens(client, auth_code, &pkce.verifier).await?;
            let api_key = obtain_api_key(client, &tokens.id_token).await.ok();
            return Ok(AuthData::chatgpt(api_key, tokens));
        }

        let status = resp.status().as_u16();
        if status != 403 && status != 404 {
            let body = resp.text().await.unwrap_or_default();
            return Err(OAuthError::Http { status, body });
        }
        // 403/404 = 用户还没授权，继续轮询
    }

    Err(OAuthError::Auth("device code flow timed out".into()))
}

/// 等待 OAuth2 回调。
async fn wait_for_callback(
    listener: &tokio::net::TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> Result<String, OAuthError> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())
            .ok_or_else(|| OAuthError::Server("callback timed out".into()))?;

        let (mut stream, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .map_err(|_| OAuthError::Server("accept timed out".into()))?
            .map_err(|e| OAuthError::Server(format!("accept failed: {e}")))?;

        // 读取 HTTP 请求行
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await
            .map_err(|e| OAuthError::Server(format!("read failed: {e}")))?;
        let request = String::from_utf8_lossy(&buf[..n]);

        // 解析请求行：GET /auth/callback?code=xxx&state=yyy HTTP/1.1
        let request_line = request.lines().next().unwrap_or("");
        let path = request_line.split_whitespace().nth(1).unwrap_or("");

        // 解析 query 参数
        let query_str = path.split('?').nth(1).unwrap_or("");
        let params: HashMap<&str, &str> = query_str
            .split('&')
            .filter_map(|pair| {
                let mut iter = pair.splitn(2, '=');
                Some((iter.next()?, iter.next()?))
            })
            .collect();

        // 返回 HTML 响应
        use tokio::io::AsyncWriteExt;
        let (status_html, status_msg) = if params.get("error").is_some() {
            let err = params.get("error_description")
                .or(params.get("error"))
                .copied()
                .unwrap_or("unknown error");
            (ERR_PAGE.replace("{MSG}", err), Err(OAuthError::Auth(err.into())))
        } else if params.get("state").copied() != Some(expected_state) {
            (ERR_PAGE.replace("{MSG}", "invalid state"), Err(OAuthError::Auth("invalid OAuth state".into())))
        } else if let Some(code) = params.get("code").copied() {
            (OK_PAGE.to_string(), Ok(code.to_string()))
        } else {
            (ERR_PAGE.replace("{MSG}", "missing code"), Err(OAuthError::Auth("missing authorization code".into())))
        };

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status_html.len(),
            status_html,
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.flush().await;

        return status_msg;
    }
}

/// 用 authorization code 换 token。
async fn exchange_code_for_tokens(
    client:  &Client,
    code:    &str,
    verifier: &str,
) -> Result<TokenResponse, OAuthError> {
    let resp = client
        .post(format!("{ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&code={code}&redirect_uri={REDIRECT_URI}&client_id={CLIENT_ID}&code_verifier={verifier}"
        ))
        .timeout(TOKEN_TIMEOUT)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http { status, body });
    }

    Ok(resp.json().await?)
}

/// 用 id_token 通过 Token Exchange（RFC 8693）换取 OpenAI API key。
async fn obtain_api_key(client: &Client, id_token: &str) -> Result<String, OAuthError> {
    let resp = client
        .post(format!("{ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=urn:ietf:params:oauth:grant-type:token-exchange\
             &requested_token=openai-api-key\
             &subject_token={id_token}\
             &subject_token_type=urn:ietf:params:oauth:token-type:id_token\
             &client_id={CLIENT_ID}"
        ))
        .timeout(TOKEN_TIMEOUT)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http { status, body });
    }

    let data: ApiKeyExchangeResponse = resp.json().await?;
    Ok(data.access_token)
}

/// 用 refresh_token 刷新 token。
pub async fn refresh_token(client: &Client, refresh: &str) -> Result<TokenResponse, OAuthError> {
    let resp = client
        .post(format!("{ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&refresh_token={refresh}&client_id={CLIENT_ID}"
        ))
        .timeout(TOKEN_TIMEOUT)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::Http { status, body });
    }

    Ok(resp.json().await?)
}

// ── 持久化 ────────────────────────────────────────────────────────────────────

/// auth.json 的存储路径：~/.config/neko/auth.json
pub fn auth_file_path() -> PathBuf {
    neko_core::session::paths::config_dir().join("auth.json")
}

/// 从磁盘加载认证数据。
pub async fn load_auth() -> Option<AuthData> {
    let path = auth_file_path();
    let raw = tokio::fs::read_to_string(&path).await.ok()?;
    serde_json::from_str(&raw).ok()
}

/// 将认证数据持久化到磁盘（权限 0600）。
pub async fn save_auth(data: &AuthData) -> Result<(), OAuthError> {
    let path = auth_file_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let json = serde_json::to_vec_pretty(data)?;
    tokio::fs::write(&path, &json).await?;

    // 设置文件权限 0600（仅 Unix）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&path).await?.permissions();
        perms.set_mode(0o600);
        tokio::fs::set_permissions(&path, perms).await?;
    }

    info!(path = %path.display(), "auth data saved");
    Ok(())
}

/// 从磁盘删除认证数据。
pub async fn clear_auth() -> Result<(), OAuthError> {
    let path = auth_file_path();
    if path.exists() {
        tokio::fs::remove_file(&path).await?;
    }
    Ok(())
}

/// 检查是否已认证（auth.json 存在且有 API key）。
pub async fn is_authenticated() -> bool {
    load_auth().await
        .map(|d| d.api_key().is_some())
        .unwrap_or(false)
}

// ── 回调 HTML 页面 ────────────────────────────────────────────────────────────

const OK_PAGE: &str = r#"<!doctype html>
<html><head><title>neko</title><meta charset="utf-8"></head>
<body style="font-family:system-ui;padding:2rem;text-align:center">
<h1>Authorization successful</h1>
<p>You can close this window.</p>
</body></html>"#;

const ERR_PAGE: &str = r#"<!doctype html>
<html><head><title>neko</title><meta charset="utf-8"></head>
<body style="font-family:system-ui;padding:2rem;text-align:center">
<h1>Authorization failed</h1>
<p>{MSG}</p>
</body></html>"#;
