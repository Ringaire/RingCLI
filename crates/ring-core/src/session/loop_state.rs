use serde::{Deserialize, Serialize};

pub const MAX_LOOP_TURNS: u32 = 50;
pub const LOOP_DONE_MARKER: &str = "【LOOP_COMPLETE】";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopState {
    pub goal: String,
    pub max_turns: u32,
    pub current_turn: u32,
    pub started_at: i64,
}

impl LoopState {
    pub fn new(goal: String, max_turns: u32) -> Self {
        Self {
            goal,
            max_turns: max_turns.min(MAX_LOOP_TURNS),
            current_turn: 0,
            started_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn advance(&mut self) {
        self.current_turn += 1;
    }

    pub fn is_exhausted(&self) -> bool {
        self.current_turn >= self.max_turns
    }

    pub fn remaining(&self) -> u32 {
        self.max_turns.saturating_sub(self.current_turn)
    }

    pub fn build_continuation_prompt(&self) -> String {
        format!(
            "[auto] Continue working toward the goal. Remaining turns: {}/{}. \
             When the goal is fully achieved, end your response with {}",
            self.remaining(),
            self.max_turns,
            LOOP_DONE_MARKER,
        )
    }

    pub fn build_system_prompt_snippet(&self) -> String {
        format!(
            "# Loop Mode\n\
             You are working in loop mode toward a goal. After each turn, you will \
             automatically continue until the goal is achieved or turns run out.\n\n\
             Goal: {}\n\
             Max turns: {}\n\n\
             Rules:\n\
             - At the end of each turn, assess progress. If the goal is fully achieved, \
             end your response with {}.\n\
             - Do NOT ask the user for confirmation or input — you are in auto-loop.\n\
             - If the goal cannot be achieved (e.g. missing dependencies, errors), \
             explain why and end with {}.\n",
            self.goal,
            self.max_turns,
            LOOP_DONE_MARKER,
            LOOP_DONE_MARKER,
        )
    }
}
