//! 子 Agent 调用 JSON 命令

use serde::{Deserialize, Serialize};
use crate::error::LinkkitResult;
use crate::tags::LinkkitTag;

/// 子 Agent 调用命令
///
/// # 示例
///
/// ```json
/// {"agent": "请分析项目结构"}
/// {"agent": "运行测试", "backend": true}
/// {"agent": "审计代码", "mode": "ask"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommand {
    /// 子 Agent 的任务提示
    pub agent: String,

    /// 是否在后台运行，默认 false（前台等待）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<bool>,

    /// 子 Agent 的权限模式：ask, edit, build
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,

    /// 子 Agent 名称（便于引用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

impl AgentCommand {
    /// 转换为 LinkkitTag::SubAgent
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        // 注意：LinkkitTag::SubAgent 不支持 bg 参数
        // backend 参数在 JSON 层面保留，但转换为 Tag 时丢弃
        Ok(LinkkitTag::SubAgent {
            prompt: self.agent,
            name: self.name,
            mode: self.mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_simple() {
        let json = r#"{"agent": "分析项目"}"#;
        let cmd: AgentCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.agent, "分析项目");
        assert_eq!(cmd.backend, None);
        assert_eq!(cmd.mode, None);
    }

    #[test]
    fn test_agent_backend() {
        let json = r#"{"agent": "长时间任务", "backend": true}"#;
        let cmd: AgentCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.agent, "长时间任务");
        assert_eq!(cmd.backend, Some(true));
    }

    #[test]
    fn test_agent_with_mode() {
        let json = r#"{
            "agent": "审计代码",
            "mode": "ask",
            "name": "审计任务"
        }"#;
        let cmd: AgentCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.agent, "审计代码");
        assert_eq!(cmd.mode, Some("ask".to_string()));
        assert_eq!(cmd.name, Some("审计任务".to_string()));
    }

    #[test]
    fn test_into_tag() {
        let cmd = AgentCommand {
            agent: "测试".to_string(),
            backend: Some(true),
            mode: Some("edit".to_string()),
            name: Some("测试任务".to_string()),
            say: None,
        };
        
        let tag = cmd.into_tag().unwrap();
        match tag {
            LinkkitTag::SubAgent { prompt, name, mode } => {
                assert_eq!(prompt, "测试");
                assert_eq!(name, Some("测试任务".to_string()));
                assert_eq!(mode, Some("edit".to_string()));
            }
            _ => panic!("Expected SubAgent tag"),
        }
    }
}
