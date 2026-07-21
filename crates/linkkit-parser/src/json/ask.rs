//! 用户询问 JSON 命令

use serde::{Deserialize, Serialize};
use crate::error::LinkkitResult;
use crate::tags::LinkkitTag;

/// 用户询问命令
///
/// # 示例
///
/// ## 简单文本询问
/// ```json
/// {"ask": "是否继续？"}
/// ```
///
/// ## 带选项的询问
/// ```json
/// {
///     "ask": [
///         {
///             "question": "选择部署环境？",
///             "header": "环境选择",
///             "options": [
///                 {"label": "dev", "description": "开发环境"},
///                 {"label": "prod", "description": "生产环境"}
///             ],
///             "multiple": false
///         }
///     ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskCommand {
    /// 询问内容：可以是简单字符串或问题数组
    pub ask: AskContent,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

/// 询问内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AskContent {
    /// 简单文本询问
    Text(String),
    
    /// 结构化问题列表
    Questions(Vec<Question>),
}

/// 问题定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    /// 问题文本
    pub question: String,
    
    /// 问题标题（简短）
    pub header: String,
    
    /// 选项列表
    pub options: Vec<QuestionOption>,
    
    /// 是否允许多选，默认 false（单选）
    #[serde(default)]
    pub multiple: bool,
}

/// 问题选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    /// 选项标签
    pub label: String,
    
    /// 选项描述
    pub description: String,
}

impl AskCommand {
    /// 转换为 LinkkitTag::Ask
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        match self.ask {
            // 简单文本询问
            AskContent::Text(text) => Ok(LinkkitTag::Ask {
                question: text,
                options: None,
            }),
            
            // 结构化问题：转换为逗号分隔的选项字符串
            AskContent::Questions(questions) => {
                if questions.is_empty() {
                    return Ok(LinkkitTag::Ask {
                        question: "（空问题）".to_string(),
                        options: None,
                    });
                }
                
                // 取第一个问题（LinkkitTag::Ask 目前只支持单个问题）
                let q = &questions[0];
                let options_str = q.options.iter()
                    .map(|opt| opt.label.clone())
                    .collect::<Vec<_>>()
                    .join(",");
                
                Ok(LinkkitTag::Ask {
                    question: q.question.clone(),
                    options: Some(options_str),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ask_simple_text() {
        let json = r#"{"ask": "是否继续？"}"#;
        let cmd: AskCommand = serde_json::from_str(json).unwrap();
        
        match cmd.ask {
            AskContent::Text(text) => assert_eq!(text, "是否继续？"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_ask_with_options() {
        let json = r#"{
            "ask": [
                {
                    "question": "选择环境？",
                    "header": "环境",
                    "options": [
                        {"label": "dev", "description": "开发"},
                        {"label": "prod", "description": "生产"}
                    ],
                    "multiple": false
                }
            ]
        }"#;
        let cmd: AskCommand = serde_json::from_str(json).unwrap();
        
        match cmd.ask {
            AskContent::Questions(questions) => {
                assert_eq!(questions.len(), 1);
                assert_eq!(questions[0].question, "选择环境？");
                assert_eq!(questions[0].options.len(), 2);
                assert!(!questions[0].multiple);
            }
            _ => panic!("Expected Questions variant"),
        }
    }

    #[test]
    fn test_into_tag_text() {
        let cmd = AskCommand {
            ask: AskContent::Text("确认吗？".to_string()),
            say: None,
        };
        
        let tag = cmd.into_tag().unwrap();
        match tag {
            LinkkitTag::Ask { question, options } => {
                assert_eq!(question, "确认吗？");
                assert_eq!(options, None);
            }
            _ => panic!("Expected Ask tag"),
        }
    }

    #[test]
    fn test_into_tag_questions() {
        let cmd = AskCommand {
            ask: AskContent::Questions(vec![
                Question {
                    question: "选择？".to_string(),
                    header: "选择".to_string(),
                    options: vec![
                        QuestionOption {
                            label: "A".to_string(),
                            description: "选项A".to_string(),
                        },
                        QuestionOption {
                            label: "B".to_string(),
                            description: "选项B".to_string(),
                        },
                    ],
                    multiple: false,
                }
            ]),
            say: None,
        };
        
        let tag = cmd.into_tag().unwrap();
        match tag {
            LinkkitTag::Ask { question, options } => {
                assert_eq!(question, "选择？");
                assert_eq!(options, Some("A,B".to_string()));
            }
            _ => panic!("Expected Ask tag"),
        }
    }
}
