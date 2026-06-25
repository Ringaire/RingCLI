use neko_providers::provider::StopReason;

#[derive(Debug, Clone)]
pub enum TurnResult {
    Continue,
    Done {
        /// 模型给出的停止原因（end_turn / tool_use / …）；保留供诊断展示
        #[allow(dead_code)]
        stop_reason: StopReason,
    },
    MaxTurns,
    Cancelled,
    Error(String),
}
