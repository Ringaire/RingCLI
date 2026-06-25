//! 额外添加的 provider 配置和实现。
//!
//! 纯 OpenAI 兼容的 provider 各有独立子目录，方便后续扩展特殊逻辑。
//! 有原生 API 的 provider（如 Ollama）也在此目录中。

pub mod ollama;
pub mod deepseek;
pub mod groq;
pub mod mistral;
pub mod together;
pub mod openrouter;
pub mod xai;
pub mod moonshot;
pub mod siliconflow;
pub mod zhipu;
pub mod baidu;
pub mod cerebras;
pub mod deepinfra;
pub mod fireworks;
pub mod perplexity;
pub mod cohere;
pub mod nvidia;
pub mod lmstudio;

pub use ollama::OllamaProvider;
