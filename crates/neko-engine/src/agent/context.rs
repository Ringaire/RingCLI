use neko_core::session::Session;
use neko_core::tools::Message;

pub struct AgentContext {
    pub messages:         Vec<Message>,
    pub system:           Option<String>,
    pub model:            String,
    pub input_tokens:     u64,
    pub output_tokens:    u64,
}

impl AgentContext {
    #[allow(dead_code)]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            messages:      Vec::new(),
            system:        None,
            model:         model.into(),
            input_tokens:  0,
            output_tokens: 0,
        }
    }

    /// 从已加载的会话构建上下文（用于 resume）。
    pub fn from_session(session: &Session, model: impl Into<String>, system: Option<String>) -> Self {
        Self {
            messages:      session.messages.clone(),
            system,
            model:         model.into(),
            input_tokens:  0,
            output_tokens: 0,
        }
    }

    pub fn add_message(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// 替换全部消息（用于上下文压缩）。
    pub fn replace_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }
}
