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
pub mod huggingface;
pub mod minimax;
pub mod minimax_cn;
pub mod qwen;
pub mod qwen_cn;
pub mod xiaomi;
pub mod xiaomi_ams;
pub mod xiaomi_sgp;
pub mod zai;
pub mod zai_cn;
pub mod ant_ling;
pub mod cloudflare_workers;
pub mod cloudflare_gateway;
pub mod vercel_gateway;
pub mod opencode;

pub use ollama::OllamaProvider;
