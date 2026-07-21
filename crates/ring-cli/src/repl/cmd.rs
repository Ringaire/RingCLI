use anyhow::Result;

/// `--list-sessions` CLI 入口；复用统一的会话列表格式化。
pub async fn list_sessions() -> Result<()> {
    super::commands::list_sessions().await
}
