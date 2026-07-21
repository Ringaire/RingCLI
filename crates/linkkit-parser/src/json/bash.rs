//! Bash 执行与管理 JSON 命令

use serde::{Deserialize, Serialize};
use crate::error::LinkkitResult;
use crate::tags::LinkkitTag;

/// Bash 执行命令
///
/// # 示例
///
/// ```json
/// {"bash": "ls -la"}
/// {"bash": "npm run dev", "backend": true}
/// {"bash": "cargo build", "say": "构建项目"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashCommand {
    /// 要执行的命令
    pub bash: String,

    /// 是否在后台运行，返回 Task ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<bool>,

    /// 超时时间（如 "120s", "5m"）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,

    /// 只返回最后 N 行
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<usize>,

    /// 在指定目录执行
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

impl BashCommand {
    /// 转换为 LinkkitTag::Bash
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        Ok(LinkkitTag::Bash {
            command: self.bash,
            timeout: parse_timeout(self.timeout.as_deref()),
            tail: self.tail,
            bg: self.backend.unwrap_or(false),
            at: self.at.map(std::path::PathBuf::from),
        })
    }
}

/// 解析超时时间（支持 "120s", "5m", "2h" 或裸数字毫秒）
fn parse_timeout(s: Option<&str>) -> Option<u64> {
    s.and_then(|s| {
        if let Some(stripped) = s.strip_suffix('s') {
            stripped.parse().ok()
        } else if let Some(stripped) = s.strip_suffix('m') {
            stripped.parse::<u64>().ok().map(|m| m * 60)
        } else if let Some(stripped) = s.strip_suffix('h') {
            stripped.parse::<u64>().ok().map(|h| h * 3600)
        } else {
            // 裸数字按毫秒处理，转换为秒
            s.parse::<u64>().ok().map(|ms| (ms + 999) / 1000)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_simple() {
        let json = r#"{"bash": "ls -la"}"#;
        let cmd: BashCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.bash, "ls -la");
        assert_eq!(cmd.backend, None);
        assert_eq!(cmd.timeout, None);
    }

    #[test]
    fn test_bash_backend() {
        let json = r#"{"bash": "npm run dev", "backend": true}"#;
        let cmd: BashCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.bash, "npm run dev");
        assert_eq!(cmd.backend, Some(true));
    }

    #[test]
    fn test_bash_full() {
        let json = r#"{
            "bash": "cargo build",
            "timeout": "5m",
            "tail": 50,
            "at": "/tmp",
            "say": "构建项目"
        }"#;
        let cmd: BashCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.bash, "cargo build");
        assert_eq!(cmd.timeout, Some("5m".to_string()));
        assert_eq!(cmd.tail, Some(50));
        assert_eq!(cmd.at, Some("/tmp".to_string()));
    }

    #[test]
    fn test_parse_timeout() {
        assert_eq!(parse_timeout(Some("60s")), Some(60));
        assert_eq!(parse_timeout(Some("5m")), Some(300));
        assert_eq!(parse_timeout(Some("2h")), Some(7200));
        assert_eq!(parse_timeout(Some("120000")), Some(120)); // 毫秒转秒
        assert_eq!(parse_timeout(None), None);
    }

    #[test]
    fn test_into_tag() {
        let cmd = BashCommand {
            bash: "ls".to_string(),
            backend: Some(true),
            timeout: Some("60s".to_string()),
            tail: None,
            at: None,
            say: None,
        };
        
        let tag = cmd.into_tag().unwrap();
        match tag {
            LinkkitTag::Bash { command, timeout, bg, .. } => {
                assert_eq!(command, "ls");
                assert_eq!(timeout, Some(60));
                assert_eq!(bg, true);
            }
            _ => panic!("Expected Bash tag"),
        }
    }
}
