//! SkillRegistry 与 MCP prompts 的双向适配层。
//!
//! # 架构定位
//!
//! MCP 规范的 `prompts/*` 与 nekocli 的 `Skill` 语义同构——都是「用户显式选择的
//! 模板化指令」（典型如 slash 命令）。本模块提供两个方向的适配：
//!
//! - [`SkillPromptProvider`]：把内部 `SkillRegistry` 以 MCP prompts 语义暴露
//!   （in-process，不走 Transport），用于统一发现接口。
//! - [`import_external_prompts`]：把外部 MCP server 的 prompts 注入 `SkillRegistry`，
//!   让第三方 prompt 与内置 skill 同等可用。
//!
//! # 不走 Transport
//!
//! 本模块是纯 Rust 内存适配，不经过 JSON-RPC 序列化。
//! [`SkillPromptProvider`] 直接读 `SkillRegistry`，返回已反序列化的 Rust 类型。
//! 这样内置 skills 与外部 MCP server 的 prompts 在「发现层」统一，
//! 但内置路径零序列化开销。

use std::sync::Arc;

use parking_lot::RwLock;

use neko_core::skills::{Skill, SkillRegistry, SkillSource};

use crate::protocol::{McpContent, McpGetPromptResult, McpPrompt, McpPromptMessage};

/// 把内部 `SkillRegistry` 以 MCP prompts 语义暴露（in-process 适配器）。
///
/// 读 `SkillRegistry`，将其中的 `Skill` 映射为 MCP `McpPrompt` / `McpGetPromptResult`。
/// 不走 Transport——直接返回 Rust 类型，零序列化开销。
pub struct SkillPromptProvider {
    registry: Arc<RwLock<SkillRegistry>>,
}

impl SkillPromptProvider {
    pub fn new(registry: Arc<RwLock<SkillRegistry>>) -> Self {
        Self { registry }
    }

    /// 等价于 `prompts/list`：列出所有 skill 作为 MCP Prompt 定义。
    ///
    /// 仅暴露 `slash: true` 的 skill（用户可通过 `/name` 触发的）。
    /// 非 slash 的 skill 是「背景知识」，不作为可选 prompt 暴露。
    pub fn list_prompts(&self) -> Vec<McpPrompt> {
        self.registry.read().list().into_iter()
            .filter(|s| s.slash)
            .map(skill_to_prompt)
            .collect()
    }

    /// 等价于 `prompts/get`：获取指定 skill 的内容作为消息序列。
    ///
    /// 当前 MVP 不支持参数化模板（忽略 `arguments`），直接返回 skill content。
    /// 未来可扩展为模板替换（如 `{target}` 占位符）。
    pub fn get_prompt(&self, name: &str) -> Result<McpGetPromptResult, PromptNotFound> {
        let reg = self.registry.read();
        let skill = reg.get(name).ok_or(PromptNotFound { name: name.to_string() })?;
        Ok(skill_to_prompt_result(skill))
    }
}

/// 错误：请求的 prompt/skill 不存在。
#[derive(Debug, thiserror::Error)]
#[error("prompt not found: {name}")]
pub struct PromptNotFound {
    pub name: String,
}

/// 把外部 MCP server 的 prompts 导入 `SkillRegistry`。
///
/// 让第三方 server 提供的 prompts 与内置 skill 同等可用（统一 `/slash` 命令发现）。
/// 每个 MCP prompt 转为一个 `Skill`（source = `SkillSource::Mcp`）。
pub async fn import_external_prompts(
    registry: &Arc<RwLock<SkillRegistry>>,
    client: &crate::McpClient,
) -> Result<usize, crate::McpError> {
    let prompts = client.prompts();
    let mut count = 0;
    for p in prompts {
        // 仅导入有 description 的 prompt（无描述的不可发现）
        let description = match &p.description {
            Some(d) if !d.is_empty() => d.clone(),
            _ => continue,
        };
        let skill = Skill {
            name: p.name.clone(),
            description,
            content: String::new(), // 延迟加载：get_prompt 时才拉取
            tools: Vec::new(),
            source: SkillSource::Mcp,
            location: None,
            slash: true,
        };
        registry.write().register(skill);
        count += 1;
    }
    tracing::debug!(imported = count, "imported MCP prompts as skills");
    Ok(count)
}

// ── 内部映射函数 ──────────────────────────────────────────────────────────────

fn skill_to_prompt(skill: &Skill) -> McpPrompt {
    McpPrompt {
        name:        skill.name.clone(),
        title:       None,
        description: if skill.description.is_empty() { None } else { Some(skill.description.clone()) },
        arguments:   Vec::new(), // MVP：Skill 无参数化
    }
}

fn skill_to_prompt_result(skill: &Skill) -> McpGetPromptResult {
    McpGetPromptResult {
        description: if skill.description.is_empty() { None } else { Some(skill.description.clone()) },
        messages: vec![McpPromptMessage {
            role:    "user".to_string(),
            content: McpContent::Text { text: skill.content.clone() },
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str, desc: &str, content: &str, slash: bool) -> Skill {
        Skill {
            name: name.to_string(),
            description: desc.to_string(),
            content: content.to_string(),
            tools: Vec::new(),
            source: SkillSource::Builtin,
            location: None,
            slash,
        }
    }

    #[test]
    fn list_prompts_only_exposes_slash_skills() {
        let mut reg = SkillRegistry::new();
        reg.register(make_skill("visible", "公开 skill", "内容", true));
        reg.register(make_skill("hidden", "非 slash skill", "内容", false));
        let provider = SkillPromptProvider::new(Arc::new(RwLock::new(reg)));

        let prompts = provider.list_prompts();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "visible");
    }

    #[test]
    fn get_prompt_returns_content_as_user_message() {
        let mut reg = SkillRegistry::new();
        reg.register(make_skill("test", "测试", "# Test\n正文", true));
        let provider = SkillPromptProvider::new(Arc::new(RwLock::new(reg)));

        let result = provider.get_prompt("test").unwrap();
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, "user");
        match &result.messages[0].content {
            McpContent::Text { text } => assert!(text.contains("正文")),
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn get_prompt_missing_returns_error() {
        let reg = SkillRegistry::new();
        let provider = SkillPromptProvider::new(Arc::new(RwLock::new(reg)));

        let err = provider.get_prompt("nonexistent").unwrap_err();
        assert_eq!(err.name, "nonexistent");
    }
}
