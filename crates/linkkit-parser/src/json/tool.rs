//! 工具调用 JSON 命令

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::error::{LinkkitError, LinkkitResult};
use crate::tags::{LinkkitTag, ToolArgs};

/// 工具调用命令
///
/// # 示例
///
/// ```json
/// {"use": "web_fetch", "meta": "https://example.com"}
/// {"use": "bash", "meta": {"command": "ls", "timeout": "10s"}}
/// {"use": "doc-ls", "say": "列出所有文档"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    /// 工具名称
    #[serde(rename = "use")]
    pub use_: String,

    /// 工具参数：可以是字符串、对象或 null
    #[serde(default = "default_meta")]
    pub meta: Value,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

fn default_meta() -> Value {
    Value::Null
}

impl ToolUse {
    /// 转换为 LinkkitTag
    ///
    /// 根据工具名称映射到具体的标签类型
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        // 特殊工具名称映射到专用标签
        match self.use_.as_str() {
            // 文档管理
            "doc-ls" => Ok(LinkkitTag::DocLs),
            
            // 工具管理
            "tool-ls" => {
                let profile = extract_bool_arg(&self.meta, "profile").unwrap_or(false);
                Ok(LinkkitTag::ToolLs { profile })
            }
            "tool-reload" => Ok(LinkkitTag::ToolReload),
            
            // TODO 管理
            "todo-clear" => Ok(LinkkitTag::TodoClear),
            "todo-done" => Ok(LinkkitTag::TodoDone),
            
            // Bash 管理
            "bash-ls" => {
                let all = extract_bool_arg(&self.meta, "all").unwrap_or(false);
                let find = extract_string_arg(&self.meta, "find");
                Ok(LinkkitTag::BashLs { all, find })
            }
            
            // 子 Agent 管理
            "sub-task" => {
                let all = extract_bool_arg(&self.meta, "all").unwrap_or(false);
                let find = extract_string_arg(&self.meta, "find");
                Ok(LinkkitTag::SubTask { all, find })
            }
            
            // 事件管理
            "event-ls" => {
                let all = extract_bool_arg(&self.meta, "all").unwrap_or(false);
                let find = extract_string_arg(&self.meta, "find");
                Ok(LinkkitTag::EventLs { all, find })
            }
            
            // 通用工具调用
            _ => {
                let args = convert_meta_to_args(self.meta)?;
                Ok(LinkkitTag::ToolUse {
                    name: self.use_,
                    args,
                })
            }
        }
    }
}

/// 将 JSON Value 转换为 ToolArgs
fn convert_meta_to_args(meta: Value) -> LinkkitResult<ToolArgs> {
    match meta {
        Value::String(s) => Ok(ToolArgs::Single(s)),
        Value::Object(map) => {
            let mut result = HashMap::new();
            for (k, v) in map {
                let value_str = match v {
                    Value::String(s) => s,
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => String::new(),
                    other => serde_json::to_string(&other)?,
                };
                result.insert(k, value_str);
            }
            Ok(ToolArgs::Multiple(result))
        }
        Value::Null => Ok(ToolArgs::Multiple(HashMap::new())),
        other => Err(LinkkitError::Other(format!(
            "无法将 meta 转换为 ToolArgs: {}",
            other
        ))),
    }
}

/// 从 Value 中提取布尔参数
fn extract_bool_arg(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| v.as_bool())
}

/// 从 Value 中提取字符串参数
fn extract_string_arg(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_use_with_string_meta() {
        let json = r#"{"use": "web_fetch", "meta": "https://example.com"}"#;
        let cmd: ToolUse = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.use_, "web_fetch");
        assert!(cmd.meta.is_string());
    }

    #[test]
    fn test_tool_use_with_object_meta() {
        let json = r#"{"use": "bash", "meta": {"command": "ls", "timeout": "10s"}}"#;
        let cmd: ToolUse = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.use_, "bash");
        assert!(cmd.meta.is_object());
    }

    #[test]
    fn test_tool_use_doc_ls() {
        let json = r#"{"use": "doc-ls"}"#;
        let cmd: ToolUse = serde_json::from_str(json).unwrap();
        let tag = cmd.into_tag().unwrap();
        
        assert!(matches!(tag, LinkkitTag::DocLs));
    }

    #[test]
    fn test_tool_use_tool_ls() {
        let json = r#"{"use": "tool-ls", "meta": {"profile": true}}"#;
        let cmd: ToolUse = serde_json::from_str(json).unwrap();
        let tag = cmd.into_tag().unwrap();
        
        match tag {
            LinkkitTag::ToolLs { profile } => assert!(profile),
            _ => panic!("Expected ToolLs tag"),
        }
    }

    #[test]
    fn test_tool_use_generic() {
        let json = r#"{"use": "custom_tool", "meta": {"arg1": "value1"}}"#;
        let cmd: ToolUse = serde_json::from_str(json).unwrap();
        let tag = cmd.into_tag().unwrap();
        
        match tag {
            LinkkitTag::ToolUse { name, args } => {
                assert_eq!(name, "custom_tool");
                match args {
                    ToolArgs::Multiple(map) => {
                        assert_eq!(map.get("arg1"), Some(&"value1".to_string()));
                    }
                    _ => panic!("Expected Multiple args"),
                }
            }
            _ => panic!("Expected ToolUse tag"),
        }
    }

    #[test]
    fn test_convert_meta_string() {
        let meta = Value::String("test".to_string());
        let args = convert_meta_to_args(meta).unwrap();
        assert!(matches!(args, ToolArgs::Single(_)));
    }

    #[test]
    fn test_convert_meta_object() {
        let json = r#"{"key": "value", "num": 42, "bool": true}"#;
        let meta: Value = serde_json::from_str(json).unwrap();
        let args = convert_meta_to_args(meta).unwrap();
        
        match args {
            ToolArgs::Multiple(map) => {
                assert_eq!(map.get("key"), Some(&"value".to_string()));
                assert_eq!(map.get("num"), Some(&"42".to_string()));
                assert_eq!(map.get("bool"), Some(&"true".to_string()));
            }
            _ => panic!("Expected Multiple args"),
        }
    }
}
