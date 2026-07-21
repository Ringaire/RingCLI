use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BashStream {
    Stdout,
    Stderr,
}

// ── 事件类型 ──────────────────────────────────────────────────────────────────
//
// 主/次（orchestrator/sub-agent）通过 `sub_agent_id` 区分：
//   - None  → 主 agent
//   - Some  → 由 spawn_agent 派生的子 agent；前端据此分层渲染

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NekoEvent {
    // ── Agent / LLM 事件 ──
    AgentThinking {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
    },
    AgentReasoning {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        delta: String,
    },
    AgentReasoningDone {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        full: String,
    },
    AgentText {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        delta: String,
    },
    AgentTextDone {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        full: String,
    },
    AgentToolCall {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        call_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    AgentError {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        error: String,
    },
    AgentDone {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        stop_reason: String,
    },
    /// 子 agent 派生：携带角色/模型/任务元数据
    AgentSpawned {
        session_id: Uuid,
        sub_agent_id: Uuid,
        role: Option<String>,
        model: String,
        task: String,
    },

    // ── 工具事件 ──
    ToolStart {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        call_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    ToolEnd {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        call_id: String,
        tool_name: String,
        ok: bool,
        duration_ms: u64,
    },
    ToolPermission {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        call_id: String,
        tool_name: String,
    },

    // ── Bash 实时输出 ──
    BashOutput {
        session_id: Uuid,
        sub_agent_id: Option<Uuid>,
        call_id: String,
        stream: BashStream,
        data: String,
    },

    // ── 会话事件 ──
    SessionStart {
        session_id: Uuid,
        cwd: String,
    },
    SessionEnd {
        session_id: Uuid,
        reason: String,
    },
    SessionMessage {
        session_id: Uuid,
        role: String,
        content: String,
    },

    // ── 上下文事件 ──
    ContextUpdate {
        session_id: Uuid,
        tokens: u64,
        message_count: usize,
    },
    ContextTruncate {
        session_id: Uuid,
        removed_messages: usize,
        strategy: String,
    },
    ContextSummary {
        session_id: Uuid,
        summary: String,
        replaced_messages: usize,
    },

    // ── 进程生命周期事件 ──
    ProcessReady {
        session_id: Uuid,
        pid: u32,
        manager_id: String,
    },
    ProcessExit {
        session_id: Uuid,
        pid: u32,
        manager_id: String,
        code: Option<i32>,
        signal: Option<String>,
    },
}

impl NekoEvent {
    /// 该事件所属会话。
    pub fn session_id(&self) -> Uuid {
        match self {
            Self::AgentThinking { session_id, .. }
            | Self::AgentReasoning { session_id, .. }
            | Self::AgentReasoningDone { session_id, .. }
            | Self::AgentText { session_id, .. }
            | Self::AgentTextDone { session_id, .. }
            | Self::AgentToolCall { session_id, .. }
            | Self::AgentError { session_id, .. }
            | Self::AgentDone { session_id, .. }
            | Self::AgentSpawned { session_id, .. }
            | Self::ToolStart { session_id, .. }
            | Self::ToolEnd { session_id, .. }
            | Self::ToolPermission { session_id, .. }
            | Self::BashOutput { session_id, .. }
            | Self::SessionStart { session_id, .. }
            | Self::SessionEnd { session_id, .. }
            | Self::SessionMessage { session_id, .. }
            | Self::ContextUpdate { session_id, .. }
            | Self::ContextTruncate { session_id, .. }
            | Self::ContextSummary { session_id, .. }
            | Self::ProcessReady { session_id, .. }
            | Self::ProcessExit { session_id, .. } => *session_id,
        }
    }

    /// 该事件所属的子 agent（主 agent 返回 None）。
    /// AgentSpawned 本身返回它派生出的子 agent id。
    pub fn sub_agent_id(&self) -> Option<Uuid> {
        match self {
            Self::AgentThinking { sub_agent_id, .. }
            | Self::AgentReasoning { sub_agent_id, .. }
            | Self::AgentReasoningDone { sub_agent_id, .. }
            | Self::AgentText { sub_agent_id, .. }
            | Self::AgentTextDone { sub_agent_id, .. }
            | Self::AgentToolCall { sub_agent_id, .. }
            | Self::AgentError { sub_agent_id, .. }
            | Self::AgentDone { sub_agent_id, .. }
            | Self::ToolStart { sub_agent_id, .. }
            | Self::ToolEnd { sub_agent_id, .. }
            | Self::ToolPermission { sub_agent_id, .. }
            | Self::BashOutput { sub_agent_id, .. } => *sub_agent_id,
            Self::AgentSpawned { sub_agent_id, .. } => Some(*sub_agent_id),
            _ => None,
        }
    }

    /// 是否来自子 agent。
    pub fn is_sub_agent(&self) -> bool {
        self.sub_agent_id().is_some()
    }
}

// ── EventBus ─────────────────────────────────────────────────────────────────

const BUS_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<NekoEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BUS_CAPACITY);
        Self { tx }
    }

    pub fn emit(&self, event: NekoEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<NekoEvent> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
