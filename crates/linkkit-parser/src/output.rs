//! Linkkit 输出格式生成器
//!
//! 生成统一的 `<output>` 标签，支持多种输出级别和来源标识。

use serde::{Deserialize, Serialize};

/// 输出级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputLevel {
    /// 正常输出（默认）
    Normal,
    /// 成功完成
    Done,
    /// 提示信息
    Tip,
    /// 警告
    Warn,
    /// 错误
    Error,
}

impl Default for OutputLevel {
    fn default() -> Self {
        Self::Normal
    }
}

impl OutputLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Done => "done",
            Self::Tip => "tip",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// 输出生成器
pub struct OutputGenerator;

impl OutputGenerator {
    /// 生成 XML 格式的 `<output>` 标签
    ///
    /// # Examples
    ///
    /// ```
    /// use linkkit_parser::{OutputGenerator, OutputLevel};
    ///
    /// let xml = OutputGenerator::xml(OutputLevel::Normal, None, "Hello");
    /// assert_eq!(xml, "<output>Hello</output>");
    ///
    /// let xml = OutputGenerator::xml(OutputLevel::Error, Some("bash"), "command not found");
    /// assert_eq!(xml, r#"<output level="error" from="bash">command not found</output>"#);
    /// ```
    pub fn xml(level: OutputLevel, from: Option<&str>, content: &str) -> String {
        let level_attr = if level != OutputLevel::Normal {
            format!(r#" level="{}""#, level.as_str())
        } else {
            String::new()
        };

        let from_attr = if let Some(f) = from {
            format!(r#" from="{}""#, escape_xml(f))
        } else {
            String::new()
        };

        format!(
            "<output{}{}>{}</output>",
            level_attr,
            from_attr,
            escape_xml(content)
        )
    }

    /// 生成 JSON 格式的输出
    ///
    /// # Examples
    ///
    /// ```
    /// use linkkit_parser::{OutputGenerator, OutputLevel};
    ///
    /// let json = OutputGenerator::json(OutputLevel::Error, Some("bash"), "command not found");
    /// // {"level":"error","from":"bash","content":"command not found"}
    /// ```
    pub fn json(level: OutputLevel, from: Option<&str>, content: &str) -> String {
        let obj = serde_json::json!({
            "level": level.as_str(),
            "from": from,
            "content": content,
        });
        obj.to_string()
    }

    /// 生成紧凑的纯文本输出（用于日志）
    ///
    /// # Examples
    ///
    /// ```
    /// use linkkit_parser::{OutputGenerator, OutputLevel};
    ///
    /// let text = OutputGenerator::text(OutputLevel::Error, Some("bash"), "command not found");
    /// assert_eq!(text, "[ERROR|bash] command not found");
    /// ```
    pub fn text(level: OutputLevel, from: Option<&str>, content: &str) -> String {
        let level_str = match level {
            OutputLevel::Normal => "INFO",
            OutputLevel::Done => "DONE",
            OutputLevel::Tip => "TIP",
            OutputLevel::Warn => "WARN",
            OutputLevel::Error => "ERROR",
        };

        if let Some(f) = from {
            format!("[{}|{}] {}", level_str, f, content)
        } else {
            format!("[{}] {}", level_str, content)
        }
    }
}

/// XML 转义（防止注入）
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_normal() {
        let xml = OutputGenerator::xml(OutputLevel::Normal, None, "Hello");
        assert_eq!(xml, "<output>Hello</output>");
    }

    #[test]
    fn test_xml_with_level() {
        let xml = OutputGenerator::xml(OutputLevel::Error, None, "Error occurred");
        assert_eq!(xml, r#"<output level="error">Error occurred</output>"#);
    }

    #[test]
    fn test_xml_with_from() {
        let xml = OutputGenerator::xml(OutputLevel::Normal, Some("bash"), "Output");
        assert_eq!(xml, r#"<output from="bash">Output</output>"#);
    }

    #[test]
    fn test_xml_full() {
        let xml = OutputGenerator::xml(OutputLevel::Error, Some("bash"), "command not found");
        assert_eq!(
            xml,
            r#"<output level="error" from="bash">command not found</output>"#
        );
    }

    #[test]
    fn test_xml_escape() {
        let xml = OutputGenerator::xml(OutputLevel::Normal, None, "<script>alert('xss')</script>");
        assert!(xml.contains("&lt;script&gt;"));
        assert!(!xml.contains("<script>"));
    }

    #[test]
    fn test_json_output() {
        let json = OutputGenerator::json(OutputLevel::Error, Some("bash"), "not found");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        
        assert_eq!(parsed["level"], "error");
        assert_eq!(parsed["from"], "bash");
        assert_eq!(parsed["content"], "not found");
    }

    #[test]
    fn test_text_normal() {
        let text = OutputGenerator::text(OutputLevel::Normal, None, "Hello");
        assert_eq!(text, "[INFO] Hello");
    }

    #[test]
    fn test_text_with_from() {
        let text = OutputGenerator::text(OutputLevel::Error, Some("bash"), "error");
        assert_eq!(text, "[ERROR|bash] error");
    }

    #[test]
    fn test_all_levels() {
        assert_eq!(OutputLevel::Normal.as_str(), "normal");
        assert_eq!(OutputLevel::Done.as_str(), "done");
        assert_eq!(OutputLevel::Tip.as_str(), "tip");
        assert_eq!(OutputLevel::Warn.as_str(), "warn");
        assert_eq!(OutputLevel::Error.as_str(), "error");
    }
}
