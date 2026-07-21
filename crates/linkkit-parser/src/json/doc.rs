//! 文档管理 JSON 命令

use serde::{Deserialize, Serialize};
use crate::error::LinkkitResult;
use crate::tags::LinkkitTag;

/// 文档阅读命令
///
/// # 示例
///
/// ```json
/// {"doc": "bash", "line": "1-50", "say": "查看 bash 用法"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocRead {
    /// 工具/文档名称
    pub doc: String,

    /// 行数范围，如 "1-50"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

impl DocRead {
    /// 转换为 LinkkitTag::DocRead
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        Ok(LinkkitTag::DocRead {
            name: Some(self.doc),
            line: self.line,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_read_full() {
        let json = r#"{"doc": "bash", "line": "1-50", "say": "查看用法"}"#;
        let cmd: DocRead = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.doc, "bash");
        assert_eq!(cmd.line, Some("1-50".to_string()));
        assert_eq!(cmd.say, Some("查看用法".to_string()));
    }

    #[test]
    fn test_doc_read_minimal() {
        let json = r#"{"doc": "web_fetch"}"#;
        let cmd: DocRead = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.doc, "web_fetch");
        assert_eq!(cmd.line, None);
        assert_eq!(cmd.say, None);
    }

    #[test]
    fn test_into_tag() {
        let cmd = DocRead {
            doc: "bash".to_string(),
            line: Some("1-50".to_string()),
            say: None,
        };
        
        let tag = cmd.into_tag().unwrap();
        match tag {
            LinkkitTag::DocRead { name, line } => {
                assert_eq!(name, Some("bash".to_string()));
                assert_eq!(line, Some("1-50".to_string()));
            }
            _ => panic!("Expected DocRead tag"),
        }
    }
}
