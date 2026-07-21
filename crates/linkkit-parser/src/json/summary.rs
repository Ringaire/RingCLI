//! 总结命令 JSON 命令

use serde::{Deserialize, Serialize};
use crate::error::LinkkitResult;
use crate::tags::LinkkitTag;

/// 总结命令
///
/// # 示例
///
/// ```json
/// {"summary": "本次会话完成了以下工作：..."}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryCommand {
    /// 总结内容
    pub summary: String,
}

impl SummaryCommand {
    /// 转换为 LinkkitTag
    ///
    /// 注意：当前 LinkkitTag 没有专门的 Summary 标签，
    /// 可能需要使用其他机制处理（如记录到日志或上下文）
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        // 目前没有对应的 LinkkitTag，使用占位实现
        // 实际使用时可能需要扩展 LinkkitTag 枚举
        Ok(LinkkitTag::DocRead {
            name: Some(format!("summary: {}", self.summary)),
            line: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_simple() {
        let json = r#"{"summary": "完成了功能实现"}"#;
        let cmd: SummaryCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.summary, "完成了功能实现");
    }

    #[test]
    fn test_summary_multiline() {
        let json = r#"{"summary": "完成了以下工作：\n1. 分析需求\n2. 编写代码"}"#;
        let cmd: SummaryCommand = serde_json::from_str(json).unwrap();
        
        assert!(cmd.summary.contains("分析需求"));
        assert!(cmd.summary.contains("编写代码"));
    }
}
