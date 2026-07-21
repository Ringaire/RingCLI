//! 事件订阅 JSON 命令

use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::error::LinkkitResult;
use crate::tags::LinkkitTag;

/// 事件订阅命令
///
/// # 示例
///
/// ## 订阅 Bash 任务完成
/// ```json
/// {"event": "task_id"}
/// ```
///
/// ## 定时事件
/// ```json
/// {"event": "time", "arg": {"time": "120s"}}
/// {"event": "time", "arg": {"clock": "11:45"}}
/// {"event": "time", "arg": {"day": "2026.5.20", "clock": "11:45"}}
/// {"event": "time", "arg": {"everytime": "2h", "max": "12"}}
/// ```
///
/// ## 进程监听
/// ```json
/// {"event": "pid", "arg": {"pid": "8821"}}
/// ```
///
/// ## 文件变化监听
/// ```json
/// {"event": "file", "arg": {"path": "src/", "max": "20"}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventCommand {
    /// 事件类型或 Task ID
    pub event: String,

    /// 事件参数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg: Option<Value>,

    /// 事件名称（便于引用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

impl EventCommand {
    /// 转换为 LinkkitTag::Event
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        let form = self.event.clone();
        
        // 根据事件类型解析参数
        let (task, time, clock, day, everytime, pid, path, shell, max) = match form.as_str() {
            "bash" | "agent" => {
                // 订阅后台任务
                let task = extract_string(&self.arg, "task")
                    .or_else(|| Some(self.event.clone()));
                (task, None, None, None, None, None, None, None, None)
            }
            "time" => {
                // 时间事件
                let time = extract_string(&self.arg, "time");
                let clock = extract_string(&self.arg, "clock");
                let day = extract_string(&self.arg, "day");
                let everytime = extract_string(&self.arg, "everytime");
                let max = extract_usize(&self.arg, "max");
                (None, time, clock, day, everytime, None, None, None, max)
            }
            "pid" => {
                // 进程事件
                let pid = extract_u32(&self.arg, "pid");
                (None, None, None, None, None, pid, None, None, None)
            }
            "file" => {
                // 文件变化事件
                let path = extract_pathbuf(&self.arg, "path");
                let max = extract_usize(&self.arg, "max");
                (None, None, None, None, None, None, path, None, max)
            }
            "cond" => {
                // 条件事件
                let shell = extract_string(&self.arg, "shell");
                let max = extract_usize(&self.arg, "max");
                (None, None, None, None, None, None, None, shell, max)
            }
            _ => {
                // 未知类型，尝试作为任务 ID
                (Some(self.event.clone()), None, None, None, None, None, None, None, None)
            }
        };

        Ok(LinkkitTag::Event {
            form,
            name: self.name,
            task,
            time,
            clock,
            day,
            everytime,
            pid,
            path,
            shell,
            max,
        })
    }
}

/// 从 JSON Value 中提取字符串字段
fn extract_string(value: &Option<Value>, key: &str) -> Option<String> {
    value.as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 从 JSON Value 中提取 u32 字段
fn extract_u32(value: &Option<Value>, key: &str) -> Option<u32> {
    value.as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| {
            v.as_u64().and_then(|n| u32::try_from(n).ok())
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
}

/// 从 JSON Value 中提取 usize 字段
fn extract_usize(value: &Option<Value>, key: &str) -> Option<usize> {
    value.as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| {
            v.as_u64().and_then(|n| usize::try_from(n).ok())
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
}

/// 从 JSON Value 中提取 PathBuf 字段
fn extract_pathbuf(value: &Option<Value>, key: &str) -> Option<std::path::PathBuf> {
    extract_string(value, key).map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_task_id() {
        let json = r#"{"event": "a3f2"}"#;
        let cmd: EventCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.event, "a3f2");
        assert_eq!(cmd.arg, None);
    }

    #[test]
    fn test_event_bash_task() {
        let json = r#"{"event": "bash", "arg": {"task": "a3f2"}, "name": "等构建"}"#;
        let cmd: EventCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.event, "bash");
        assert!(cmd.arg.is_some());
        assert_eq!(cmd.name, Some("等构建".to_string()));
    }

    #[test]
    fn test_event_time_relative() {
        let json = r#"{"event": "time", "arg": {"time": "120s"}}"#;
        let cmd: EventCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.event, "time");
        let time = extract_string(&cmd.arg, "time");
        assert_eq!(time, Some("120s".to_string()));
    }

    #[test]
    fn test_event_time_clock() {
        let json = r#"{"event": "time", "arg": {"clock": "11:45"}}"#;
        let cmd: EventCommand = serde_json::from_str(json).unwrap();
        
        let clock = extract_string(&cmd.arg, "clock");
        assert_eq!(clock, Some("11:45".to_string()));
    }

    #[test]
    fn test_event_time_everyday() {
        let json = r#"{
            "event": "time",
            "arg": {"day": "everyday", "clock": "11:45"}
        }"#;
        let cmd: EventCommand = serde_json::from_str(json).unwrap();
        
        let day = extract_string(&cmd.arg, "day");
        let clock = extract_string(&cmd.arg, "clock");
        assert_eq!(day, Some("everyday".to_string()));
        assert_eq!(clock, Some("11:45".to_string()));
    }

    #[test]
    fn test_event_pid() {
        let json = r#"{"event": "pid", "arg": {"pid": "8821"}}"#;
        let cmd: EventCommand = serde_json::from_str(json).unwrap();
        
        let pid = extract_string(&cmd.arg, "pid");
        assert_eq!(pid, Some("8821".to_string()));
    }

    #[test]
    fn test_event_file() {
        let json = r#"{"event": "file", "arg": {"path": "src/", "max": "20"}}"#;
        let cmd: EventCommand = serde_json::from_str(json).unwrap();
        
        let path = extract_string(&cmd.arg, "path");
        let max = extract_string(&cmd.arg, "max");
        assert_eq!(path, Some("src/".to_string()));
        assert_eq!(max, Some("20".to_string()));
    }

    #[test]
    fn test_into_tag_bash() {
        let cmd = EventCommand {
            event: "bash".to_string(),
            arg: Some(serde_json::json!({"task": "a3f2"})),
            name: Some("等构建".to_string()),
            say: None,
        };
        
        let tag = cmd.into_tag().unwrap();
        match tag {
            LinkkitTag::Event { form, name, task, .. } => {
                assert_eq!(form, "bash");
                assert_eq!(name, Some("等构建".to_string()));
                assert_eq!(task, Some("a3f2".to_string()));
            }
            _ => panic!("Expected Event tag"),
        }
    }
}
