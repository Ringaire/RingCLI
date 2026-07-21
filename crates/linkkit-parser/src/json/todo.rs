//! TODO 管理 JSON 命令

use serde::{Deserialize, Serialize};
use crate::error::LinkkitResult;
use crate::tags::LinkkitTag;

/// TODO 更新命令
///
/// # 示例
///
/// ```json
/// {
///     "todo": "[ ] 分析项目\n[ ] 实现功能\n[x] 测试",
///     "say": "更新待办列表"
/// }
/// ```
///
/// 支持的勾选框状态：
/// - `[ ]` - 未开始
/// - `[*]` - 进行中
/// - `[x]` - 已完成
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoCommand {
    /// Markdown 格式的待办内容
    pub todo: String,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

impl TodoCommand {
    /// 转换为 LinkkitTag::TodoUpdate
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        Ok(LinkkitTag::TodoUpdate {
            content: self.todo,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_todo_simple() {
        let json = r#"{"todo": "[ ] 任务1\n[x] 任务2"}"#;
        let cmd: TodoCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.todo, "[ ] 任务1\n[x] 任务2");
        assert_eq!(cmd.say, None);
    }

    #[test]
    fn test_todo_with_say() {
        let json = r#"{
            "todo": "[ ] 分析\n[ ] 实现",
            "say": "更新任务列表"
        }"#;
        let cmd: TodoCommand = serde_json::from_str(json).unwrap();
        
        assert!(cmd.todo.contains("分析"));
        assert_eq!(cmd.say, Some("更新任务列表".to_string()));
    }

    #[test]
    fn test_into_tag() {
        let cmd = TodoCommand {
            todo: "[ ] 任务".to_string(),
            say: None,
        };
        
        let tag = cmd.into_tag().unwrap();
        match tag {
            LinkkitTag::TodoUpdate { content } => {
                assert_eq!(content, "[ ] 任务");
            }
            _ => panic!("Expected TodoUpdate tag"),
        }
    }
}
