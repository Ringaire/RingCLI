use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub msg_type: MsgType,
    pub payload: serde_json::Value,
    pub timestamp: i64,
    pub direction: Direction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Upstream,
    Downstream,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MsgType {
    Register,
    RegisterAck,
    AssignTask,
    TaskResult,
    Heartbeat,
    HeartbeatAck,
    Error,
    SessionControl,
    SessionEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Register {
    pub client_id: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterAck {
    pub session_id: Uuid,
    pub heartbeat_interval_secs: u64,
    pub server_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignTask {
    pub task_id: Uuid,
    pub platform: String,
    pub platform_user_id: String,
    pub conversation_id: String,
    pub message: TaskMessage,
    pub context: Option<TaskContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMessage {
    pub text: String,
    pub attachments: Option<Vec<Attachment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub data: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    pub history: Option<Vec<serde_json::Value>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: Uuid,
    pub status: TaskStatus,
    pub output: Option<TaskOutput>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Completed,
    Failed,
    Cancelled,
    RequiresAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    pub text: String,
    pub actions: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub task_id: Option<Uuid>,
}
