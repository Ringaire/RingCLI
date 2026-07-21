// Agent 与前端之间的权限请求/响应通道

use tokio::sync::oneshot;

/// 用户对一次权限请求的决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// 仅本次允许
    AllowOnce,
    /// 永久允许该工具（写入自定义规则）
    AllowAlways,
    /// 仅本次拒绝
    DenyOnce,
    /// 永久拒绝该工具
    DenyAlways,
}

impl PermissionDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::AllowOnce | Self::AllowAlways)
    }
}

/// 一次权限请求：executor 发出，前端（TUI/plain）回应。
pub struct PermissionRequest {
    pub tool_name:     String,
    /// 关联的工具调用 id（供前端按调用关联高亮）
    #[allow(dead_code)]
    pub call_id:       String,
    pub input_preview: String,
    pub responder:     oneshot::Sender<PermissionDecision>,
}

/// 权限请求发送端类型别名。
pub type PermissionSender = tokio::sync::mpsc::Sender<PermissionRequest>;
