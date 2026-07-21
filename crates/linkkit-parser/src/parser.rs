use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{LinkkitError, LinkkitResult};
use crate::tags::{LinkkitTag, ToolArgs};

/// Linkkit XML 解析器
pub struct LinkkitParser<'a> {
    reader: Reader<&'a [u8]>,
}

impl<'a> LinkkitParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut reader = Reader::from_str(input);
        reader.config_mut().trim_text(true);
        Self { reader }
    }

    /// 解析输入，返回所有识别到的标签
    pub fn parse(&mut self) -> LinkkitResult<Vec<LinkkitTag>> {
        let mut tags = Vec::new();
        let mut buf = Vec::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let tag = self.parse_tag(&e)?;
                    tags.push(tag);
                }
                Ok(Event::Empty(e)) => {
                    let tag = self.parse_empty_tag(&e)?;
                    tags.push(tag);
                }
                Ok(Event::Eof) => break,
                Ok(_) => {} // 忽略其他事件（文本、注释等）
                Err(e) => {
                    return Err(LinkkitError::QuickXml(e));
                }
            }
            buf.clear();
        }

        Ok(tags)
    }

    /// 解析空标签（自闭合）
    fn parse_empty_tag(&self, e: &BytesStart) -> LinkkitResult<LinkkitTag> {
        let name_bytes = e.name();
        let name = std::str::from_utf8(name_bytes.as_ref())?;
        let attrs = parse_attributes(e)?;

        match name {
            "doc-ls" => Ok(LinkkitTag::DocLs),
            "doc-read" => Ok(LinkkitTag::DocRead {
                name: attrs.get("name").map(|s| s.to_string()),
                line: attrs.get("line").map(|s| s.to_string()),
            }),
            "tool-ls" => Ok(LinkkitTag::ToolLs {
                profile: attrs
                    .get("profile")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(false),
            }),
            "tool-info" => {
                let name = attrs
                    .get("name")
                    .ok_or_else(|| LinkkitError::MissingAttribute {
                        tag: name.to_string(),
                        attr: "name".to_string(),
                    })?
                    .to_string();
                Ok(LinkkitTag::ToolInfo { name })
            }
            "tool-reload" => Ok(LinkkitTag::ToolReload),
            "bash-ls" => Ok(LinkkitTag::BashLs {
                all: attrs
                    .get("all")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(false),
                find: attrs.get("find").map(|s| s.to_string()),
            }),
            "read" => {
                let file = attrs
                    .get("file")
                    .ok_or_else(|| LinkkitError::MissingAttribute {
                        tag: name.to_string(),
                        attr: "file".to_string(),
                    })?;
                Ok(LinkkitTag::Read {
                    file: PathBuf::from(file),
                    line: attrs.get("line").map(|s| s.to_string()),
                    tail: attrs.get("tail").and_then(|s| s.parse().ok()),
                })
            }
            "tree" => {
                let path = attrs
                    .get("path")
                    .ok_or_else(|| LinkkitError::MissingAttribute {
                        tag: name.to_string(),
                        attr: "path".to_string(),
                    })?;
                Ok(LinkkitTag::Tree {
                    path: PathBuf::from(path),
                    level: attrs.get("level").and_then(|s| s.parse().ok()),
                    exclude: attrs.get("exclude").map(|s| s.to_string()),
                    all: attrs
                        .get("all")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(false),
                })
            }
            "todo-done" => Ok(LinkkitTag::TodoDone),
            "todo-clear" => Ok(LinkkitTag::TodoClear),
            "sub-task" => Ok(LinkkitTag::SubTask {
                all: attrs
                    .get("all")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(false),
                find: attrs.get("find").map(|s| s.to_string()),
            }),
            "event" => {
                let form = attrs
                    .get("form")
                    .ok_or_else(|| LinkkitError::MissingAttribute {
                        tag: name.to_string(),
                        attr: "form".to_string(),
                    })?
                    .to_string();
                Ok(LinkkitTag::Event {
                    form,
                    name: attrs.get("name").map(|s| s.to_string()),
                    task: attrs.get("task").map(|s| s.to_string()),
                    pid: attrs.get("pid").and_then(|s| s.parse().ok()),
                    time: attrs.get("time").map(|s| s.to_string()),
                    clock: attrs.get("clock").map(|s| s.to_string()),
                    day: attrs.get("day").map(|s| s.to_string()),
                    everytime: attrs.get("everytime").map(|s| s.to_string()),
                    path: attrs.get("path").map(PathBuf::from),
                    shell: attrs.get("shell").map(|s| s.to_string()),
                    max: attrs.get("max").and_then(|s| s.parse().ok()),
                })
            }
            "event-ls" => Ok(LinkkitTag::EventLs {
                all: attrs
                    .get("all")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(false),
                find: attrs.get("find").map(|s| s.to_string()),
            }),
            _ => Err(LinkkitError::UnknownTag {
                tag: name.to_string(),
            }),
        }
    }

    /// 解析开始标签（需要闭合）
    fn parse_tag(&mut self, e: &BytesStart) -> LinkkitResult<LinkkitTag> {
        let name_bytes = e.name();
        let name = std::str::from_utf8(name_bytes.as_ref())?;
        let attrs = parse_attributes(e)?;

        match name {
            "bash" => self.parse_bash(attrs),
            "bash-kill" => self.parse_bash_kill(),
            "bash-log" => self.parse_bash_log(attrs),
            "edit" => self.parse_edit(attrs),
            "write" => self.parse_write(attrs),
            "web-fetch" => self.parse_web_fetch(),
            "todo-update" => self.parse_todo_update(),
            "sub-agent" => self.parse_sub_agent(attrs),
            "sub-cancel" => self.parse_sub_cancel(),
            "event-cancel" => self.parse_event_cancel(attrs),
            "ask" => self.parse_ask(attrs),
            "tool-use" => self.parse_tool_use(attrs),
            _ => Err(LinkkitError::UnknownTag {
                tag: name.to_string(),
            }),
        }
    }

    // ─── 具体标签解析 ───────────────────────────────────────────────────────

    fn parse_bash(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let content = self.read_text_until("bash")?;
        Ok(LinkkitTag::Bash {
            command: content,
            timeout: attrs.get("timeout").and_then(|s| parse_timeout(s)),
            tail: attrs.get("tail").and_then(|s| s.parse().ok()),
            bg: attrs
                .get("bg")
                .and_then(|s| s.parse().ok())
                .unwrap_or(false),
            at: attrs.get("at").map(PathBuf::from),
        })
    }

    fn parse_bash_kill(&mut self) -> LinkkitResult<LinkkitTag> {
        let task_id = self.read_text_until("bash-kill")?;
        if task_id.is_empty() {
            return Err(LinkkitError::EmptyContent {
                tag: "bash-kill".to_string(),
            });
        }
        Ok(LinkkitTag::BashKill { task_id })
    }

    fn parse_bash_log(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let task_id = self.read_text_until("bash-log")?;
        if task_id.is_empty() {
            return Err(LinkkitError::EmptyContent {
                tag: "bash-log".to_string(),
            });
        }
        Ok(LinkkitTag::BashLog {
            task_id,
            line: attrs.get("line").map(|s| s.to_string()),
            tail: attrs.get("tail").and_then(|s| s.parse().ok()),
        })
    }

    fn parse_edit(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let file = attrs
            .get("file")
            .ok_or_else(|| LinkkitError::MissingAttribute {
                tag: "edit".to_string(),
                attr: "file".to_string(),
            })?;
        let all = attrs
            .get("all")
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);

        // 检查是否有 <old>/<new> 子标签
        let (old, new, content) = self.parse_edit_content()?;

        Ok(LinkkitTag::Edit {
            file: PathBuf::from(file),
            old,
            new,
            all,
            content,
        })
    }

    fn parse_edit_content(&mut self) -> LinkkitResult<(Option<String>, Option<String>, Option<String>)> {
        let mut buf = Vec::new();
        let mut old_content = None;
        let mut new_content = None;
        let mut text_content = String::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name_bytes = e.name();
                    let name = std::str::from_utf8(name_bytes.as_ref())?;
                    match name {
                        "old" => {
                            old_content = Some(self.read_text_until("old")?);
                        }
                        "new" => {
                            new_content = Some(self.read_text_until("new")?);
                        }
                        _ => {
                            return Err(LinkkitError::UnknownTag {
                                tag: name.to_string(),
                            });
                        }
                    }
                }
                Ok(Event::Text(e)) => {
                    let t = e.unescape()?;
                    text_content.push_str(&t);
                }
                Ok(Event::End(e)) => {
                    let name_bytes = e.name();
                    let name = std::str::from_utf8(name_bytes.as_ref())?;
                    if name == "edit" {
                        break;
                    }
                }
                Ok(Event::Eof) => {
                    return Err(LinkkitError::UnclosedTag {
                        tag: "edit".to_string(),
                    });
                }
                Ok(_) => {}
                Err(e) => return Err(LinkkitError::QuickXml(e)),
            }
            buf.clear();
        }

        // 如果有 old/new，返回局部替换；否则返回整文件内容
        if old_content.is_some() || new_content.is_some() {
            Ok((old_content, new_content, None))
        } else {
            Ok((None, None, Some(text_content.trim().to_string())))
        }
    }

    fn parse_write(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let file = attrs
            .get("file")
            .ok_or_else(|| LinkkitError::MissingAttribute {
                tag: "write".to_string(),
                attr: "file".to_string(),
            })?;
        let content = self.read_text_until("write")?;
        Ok(LinkkitTag::Write {
            file: PathBuf::from(file),
            content,
        })
    }

    fn parse_web_fetch(&mut self) -> LinkkitResult<LinkkitTag> {
        let url = self.read_text_until("web-fetch")?;
        if url.is_empty() {
            return Err(LinkkitError::EmptyContent {
                tag: "web-fetch".to_string(),
            });
        }
        Ok(LinkkitTag::WebFetch { url })
    }

    fn parse_todo_update(&mut self) -> LinkkitResult<LinkkitTag> {
        let content = self.read_text_until("todo-update")?;
        Ok(LinkkitTag::TodoUpdate { content })
    }

    fn parse_sub_agent(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let prompt = self.read_text_until("sub-agent")?;
        if prompt.is_empty() {
            return Err(LinkkitError::EmptyContent {
                tag: "sub-agent".to_string(),
            });
        }
        Ok(LinkkitTag::SubAgent {
            prompt,
            name: attrs.get("name").map(|s| s.to_string()),
            mode: attrs.get("mode").map(|s| s.to_string()),
        })
    }

    fn parse_sub_cancel(&mut self) -> LinkkitResult<LinkkitTag> {
        let task_id = self.read_text_until("sub-cancel")?;
        if task_id.is_empty() {
            return Err(LinkkitError::EmptyContent {
                tag: "sub-cancel".to_string(),
            });
        }
        Ok(LinkkitTag::SubCancel { task_id })
    }

    fn parse_event_cancel(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let content = self.read_text_until("event-cancel")?;
        Ok(LinkkitTag::EventCancel {
            id: if content.is_empty() { None } else { Some(content) },
            form: attrs.get("form").map(|s| s.to_string()),
        })
    }

    fn parse_ask(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let question = self.read_text_until("ask")?;
        if question.is_empty() {
            return Err(LinkkitError::EmptyContent {
                tag: "ask".to_string(),
            });
        }
        Ok(LinkkitTag::Ask {
            question,
            options: attrs.get("options").map(|s| s.to_string()),
        })
    }

    fn parse_tool_use(&mut self, attrs: HashMap<String, String>) -> LinkkitResult<LinkkitTag> {
        let name = attrs
            .get("name")
            .ok_or_else(|| LinkkitError::MissingAttribute {
                tag: "tool-use".to_string(),
                attr: "name".to_string(),
            })?
            .to_string();

        let args = self.parse_tool_args()?;
        Ok(LinkkitTag::ToolUse { name, args })
    }

    fn parse_tool_args(&mut self) -> LinkkitResult<ToolArgs> {
        let mut buf = Vec::new();
        let mut params = HashMap::new();
        let mut text_content = String::new();
        let mut has_child_tags = false;

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    has_child_tags = true;
                    let name_bytes = e.name();
                    let param_name = std::str::from_utf8(name_bytes.as_ref())?.to_string();
                    let param_value = self.read_text_until(&param_name)?;
                    params.insert(param_name, param_value);
                }
                Ok(Event::Text(e)) => {
                    let t = e.unescape()?;
                    text_content.push_str(&t);
                }
                Ok(Event::End(e)) => {
                    let name_bytes = e.name();
                    let name = std::str::from_utf8(name_bytes.as_ref())?;
                    if name == "tool-use" {
                        break;
                    }
                }
                Ok(Event::Eof) => {
                    return Err(LinkkitError::UnclosedTag {
                        tag: "tool-use".to_string(),
                    });
                }
                Ok(_) => {}
                Err(e) => return Err(LinkkitError::QuickXml(e)),
            }
            buf.clear();
        }

        if has_child_tags {
            Ok(ToolArgs::Multiple(params))
        } else {
            Ok(ToolArgs::Single(text_content.trim().to_string()))
        }
    }

    // ─── 辅助方法 ───────────────────────────────────────────────────────────

    /// 读取文本直到遇到指定的闭合标签
    fn read_text_until(&mut self, closing_tag: &str) -> LinkkitResult<String> {
        let mut buf = Vec::new();
        let mut text = String::new();

        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Text(e)) => {
                    let t = e.unescape()?;
                    text.push_str(&t);
                }
                Ok(Event::CData(e)) => {
                    let t = std::str::from_utf8(&e)?;
                    text.push_str(t);
                }
                Ok(Event::End(e)) => {
                    let name_bytes = e.name();
                    let name = std::str::from_utf8(name_bytes.as_ref())?;
                    if name == closing_tag {
                        break;
                    } else {
                        return Err(LinkkitError::NestingMismatch {
                            expected: closing_tag.to_string(),
                            found: name.to_string(),
                        });
                    }
                }
                Ok(Event::Eof) => {
                    return Err(LinkkitError::UnclosedTag {
                        tag: closing_tag.to_string(),
                    });
                }
                Ok(_) => {}
                Err(e) => return Err(LinkkitError::QuickXml(e)),
            }
            buf.clear();
        }

        Ok(text.trim().to_string())
    }
}

// ─── 辅助函数 ───────────────────────────────────────────────────────────────

/// 解析属性为 HashMap
fn parse_attributes(e: &BytesStart) -> LinkkitResult<HashMap<String, String>> {
    let mut map = HashMap::new();
    for attr in e.attributes() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?.to_string();
        let value = attr.unescape_value()?.to_string();
        map.insert(key, value);
    }
    Ok(map)
}

/// 解析超时时间（支持 "120s", "5m", "2h" 或裸数字毫秒）
fn parse_timeout(s: &str) -> Option<u64> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_doc_ls() {
        let input = "<doc-ls/>";
        let mut parser = LinkkitParser::new(input);
        let tags = parser.parse().unwrap();
        assert_eq!(tags.len(), 1);
        assert!(matches!(tags[0], LinkkitTag::DocLs));
    }

    #[test]
    fn test_parse_bash() {
        let input = r#"<bash timeout="60s">ls -la</bash>"#;
        let mut parser = LinkkitParser::new(input);
        let tags = parser.parse().unwrap();
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            LinkkitTag::Bash { command, timeout, .. } => {
                assert_eq!(command, "ls -la");
                assert_eq!(*timeout, Some(60));
            }
            _ => panic!("Expected Bash tag"),
        }
    }

    #[test]
    fn test_parse_edit_with_old_new() {
        let input = r#"<edit file="test.rs"><old>foo</old><new>bar</new></edit>"#;
        let mut parser = LinkkitParser::new(input);
        let tags = parser.parse().unwrap();
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            LinkkitTag::Edit { file, old, new, content, .. } => {
                assert_eq!(file, &PathBuf::from("test.rs"));
                assert_eq!(old.as_deref(), Some("foo"));
                assert_eq!(new.as_deref(), Some("bar"));
                assert!(content.is_none());
            }
            _ => panic!("Expected Edit tag"),
        }
    }

    #[test]
    fn test_parse_tool_use_single() {
        let input = r#"<tool-use name="web_fetch">https://example.com</tool-use>"#;
        let mut parser = LinkkitParser::new(input);
        let tags = parser.parse().unwrap();
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            LinkkitTag::ToolUse { name, args } => {
                assert_eq!(name, "web_fetch");
                assert!(matches!(args, ToolArgs::Single(_)));
            }
            _ => panic!("Expected ToolUse tag"),
        }
    }

    #[test]
    fn test_parse_tool_use_multiple() {
        let input = r#"<tool-use name="web_fetch"><url>https://example.com</url><selector>main</selector></tool-use>"#;
        let mut parser = LinkkitParser::new(input);
        let tags = parser.parse().unwrap();
        assert_eq!(tags.len(), 1);
        match &tags[0] {
            LinkkitTag::ToolUse { name, args } => {
                assert_eq!(name, "web_fetch");
                if let ToolArgs::Multiple(map) = args {
                    assert_eq!(map.get("url").unwrap(), "https://example.com");
                    assert_eq!(map.get("selector").unwrap(), "main");
                } else {
                    panic!("Expected Multiple args");
                }
            }
            _ => panic!("Expected ToolUse tag"),
        }
    }
}
