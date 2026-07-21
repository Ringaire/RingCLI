use ring_core::skills::{Skill, SkillRegistry, SkillSource};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::warn;

// ── 参数替换引擎（PromptTemplate 可编排）────────────────────────────────────

/// 解析 bash 风格命令参数（支持双引号/单引号包裹）。
///
/// ```ignore
/// assert_eq!(parse_command_args("hello world"), vec!["hello", "world"]);
/// assert_eq!(parse_command_args(r#"foo "bar baz" qux"#), vec!["foo", "bar baz", "qux"]);
/// ```
pub fn parse_command_args(args_string: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;

    for ch in args_string.chars() {
        match in_quote {
            Some(q) if ch == q => in_quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => in_quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// 对模板内容做参数替换。
///
/// 支持语法：
/// - `$1`, `$2`, ... — 按位置引用第 N 个参数（1-indexed）
/// - `$@`, `$ARGUMENTS` — 所有参数拼接
/// - `${N:-default}` — 第 N 个参数，缺失时使用 default
/// - `${@:-default}`, `${ARGUMENTS:-default}` — 所有参数，空时使用 default
/// - `${@:N}` — 从第 N 个参数起（1-indexed，bash 风格切片）
/// - `${@:N:L}` — 从第 N 个参数起取 L 个
///
/// 注意：替换仅作用于模板文本本身，参数值和默认值中的 `$1`/`$@` 等模式**不会**被递归替换。
pub fn substitute_args(content: &str, args: &[String]) -> String {
    let all_args = args.join(" ");
    let mut result = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            // ${...} 形式
            if bytes[i + 1] == b'{' {
                if let Some(end) = content[i + 2..].find('}') {
                    let inside = &content[i + 2..i + 2 + end];
                    let replaced = match expand_braced(inside, args, &all_args) {
                        Some(s) => s,
                        None => content[i..i + 2 + end + 1].to_string(),
                    };
                    result.push_str(&replaced);
                    i += 2 + end + 1;
                    continue;
                }
            }
            // $N / $@ / $ARGUMENTS
            if bytes[i + 1].is_ascii_digit()
                || bytes[i + 1] == b'@'
                || bytes[i + 1].is_ascii_uppercase()
            {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'@')
                {
                    end += 1;
                }
                let token = &content[start..end];
                let replaced = match resolve_simple(token, args, &all_args) {
                    Some(s) => s,
                    None => content[i..end].to_string(),
                };
                result.push_str(&replaced);
                i = end;
                continue;
            }
        }
        // 普通字符
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

/// 处理 `${...}` 花括号内部的表达式。
///
/// 模式匹配优先级：
/// 1. `${N:-default}` / `${@:-default}` / `${ARGUMENTS:-default}` — 带默认值
/// 2. `${@:N}` / `${@:N:L}` / `${ARGUMENTS:N}` — 切片
/// 3. `${N}` / `${@}` / `${ARGUMENTS}` — 纯引用
fn expand_braced(inside: &str, args: &[String], all_args: &str) -> Option<String> {
    // 1. ${N:-default} / ${@:-default} / ${ARGUMENTS:-default}
    if let Some(pos) = inside.find(":-") {
        let idx_str = inside[..pos].trim();
        let default_val = &inside[pos + 2..]; // skip ":-"
        let value = resolve_positional(idx_str, args, all_args);
        if value.is_empty() {
            return Some(default_val.to_string());
        }
        return Some(value);
    }

    // 2. ${@:N} / ${@:N:L} 切片
    if let Some(slice) = inside.strip_prefix("@:") {
        return expand_slice(slice, args);
    }
    // ${ARGUMENTS:N} 切片
    if let Some(slice) = inside.strip_prefix("ARGUMENTS:") {
        return expand_slice(slice, args);
    }
    // ${N:M} 切片（纯数字）
    if let Some(slice) = inside.strip_prefix(":") {
        return expand_slice(slice, args);
    }

    // 3. 纯 ${N} / ${@} / ${ARGUMENTS}
    let value = resolve_positional(inside.trim(), args, all_args);
    if value.is_empty() {
        return None;
    }
    Some(value)
}

/// 处理 ${@:N} / ${@:N:L} 切片。
fn expand_slice(slice: &str, args: &[String]) -> Option<String> {
    let parts: Vec<&str> = slice.splitn(2, ':').collect();
    let start: usize = parts[0].trim().parse::<usize>().unwrap_or(1).saturating_sub(1);
    if start >= args.len() {
        return Some(String::new());
    }
    if parts.len() > 1 {
        let length: usize = parts[1].trim().parse().unwrap_or(args.len() - start);
        Some(args[start..start + length.min(args.len() - start)].join(" "))
    } else {
        Some(args[start..].join(" "))
    }
}

/// 解析 $N / $@ / $ARGUMENTS 简单引用。
fn resolve_simple(token: &str, args: &[String], all_args: &str) -> Option<String> {
    if token == "@" || token == "ARGUMENTS" {
        return Some(all_args.to_string());
    }
    if let Ok(idx) = token.parse::<usize>() {
        if idx >= 1 && idx <= args.len() {
            return Some(args[idx - 1].clone());
        }
    }
    None
}

/// 解析 ${N} / ${@} / ${ARGUMENTS} 花括号引用。
fn resolve_positional(token: &str, args: &[String], all_args: &str) -> String {
    if token == "@" || token == "ARGUMENTS" {
        return all_args.to_string();
    }
    if let Ok(idx) = token.parse::<usize>() {
        if idx >= 1 && idx <= args.len() {
            return args[idx - 1].clone();
        }
    }
    String::new()
}

/// 旧 JSON 格式的 skill 文件结构。
#[derive(Debug, Deserialize, Serialize)]
pub struct SkillFile {
    pub name:        String,
    pub description: String,
    pub prompt:      String,
    #[serde(default)]
    pub tools:       Vec<String>,
}

/// SKILL.md frontmatter 解析结果。
struct Frontmatter {
    name:        Option<String>,
    description: Option<String>,
    slash:       bool,
}

/// 解析 SKILL.md 的 YAML frontmatter + 正文。
/// 返回 (frontmatter, body)。
fn parse_skill_md(raw: &str) -> (Frontmatter, String) {
    let fm = Frontmatter {
        name:        None,
        description: None,
        slash:       false,
    };

    // 必须以 --- 开头
    let rest = match raw.strip_prefix("---") {
        Some(r) => r,
        None => return (fm, raw.to_string()),
    };

    // 找结束 ---
    let end = rest.find("\n---").or_else(|| rest.find("\r\n---"));
    let Some(end) = end else {
        return (fm, raw.to_string());
    };

    let frontmatter_text = &rest[..end];
    // 跳过结束 --- 和换行
    let after = &rest[end..];
    let body = after
        .trim_start_matches('-')
        .trim_start_matches(['\r', '\n'])
        .to_string();

    // 手动解析 frontmatter（只需 name、description、slash 三个字段）
    let mut name = None;
    let mut description = None;
    let mut slash = false;

    for line in frontmatter_text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
        } else if let Some(v) = line.strip_prefix("slash:") {
            slash = v.trim() == "true";
        }
    }

    (Frontmatter { name, description, slash }, body)
}

pub fn load_builtin_skills(registry: &mut SkillRegistry) {
    let builtins = vec![
        Skill {
            name:        "compact".into(),
            description: "Summarize the conversation and replace with a compact context".into(),
            content:     "Please summarize the current conversation context in a concise format, \
                         preserving key decisions, code changes, and important context. \
                         Then replace the conversation with this summary.".into(),
            tools:       vec!["bash".into(), "read_file".into()],
            source:      SkillSource::Builtin,
            location:    None,
            slash:       true,
        },
        Skill {
            name:        "review".into(),
            description: "Review recent code changes and provide feedback".into(),
            content:     "Review the recent changes in this session. Identify potential issues, \
                         improvements, and confirm the changes align with best practices.".into(),
            tools:       vec!["bash".into(), "read_file".into(), "grep".into()],
            source:      SkillSource::Builtin,
            location:    None,
            slash:       true,
        },
        Skill {
            name:        "commit".into(),
            description: "Stage and commit changes with an auto-generated message".into(),
            content:     "Review the current git diff, generate an appropriate commit message following \
                         conventional commits format, and create the commit.".into(),
            tools:       vec!["bash".into()],
            source:      SkillSource::Builtin,
            location:    None,
            slash:       true,
        },
    ];

    for skill in builtins {
        registry.register(skill);
    }
}

/// 从目录加载 skill：扫描 `*/SKILL.md`（Markdown+frontmatter）和 `*.md`（同格式）和 `*.json`（旧格式）。
pub async fn load_skills_from_dir(registry: &mut SkillRegistry, dir: &Path) {
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(r) => r,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();

        // 子目录：扫描其中的 SKILL.md
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                load_skill_md(registry, &skill_md).await;
            }
            continue;
        }

        // .md 文件：带 frontmatter 的 skill（如 engineering/*.md）
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            load_skill_md(registry, &path).await;
        }
        // .json 文件：旧格式兼容
        else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            load_skill_json(registry, &path).await;
        }
    }
}

/// 加载单个 SKILL.md 文件。
async fn load_skill_md(registry: &mut SkillRegistry, path: &Path) {
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return;
    };
    let (fm, body) = parse_skill_md(&raw);

    // name：SKILL.md 用 frontmatter/父目录名；普通 .md 文件用文件名（去扩展名）
    let is_skill_md = path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md");
    let name = if is_skill_md {
        fm.name.unwrap_or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        })
    } else {
        path.file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    // 无 description 的 skill 不注册（AI 看不到它就无法匹配）
    let Some(description) = fm.description else {
        warn!(file = %path.display(), "SKILL.md missing description, skipping");
        return;
    };

    let location = path.parent().map(|p| p.to_path_buf());

    registry.register(Skill {
        name,
        description,
        content: body,
        tools: Vec::new(),
        source: SkillSource::Filesystem,
        location,
        slash: fm.slash,
    });
}

/// 加载旧 JSON 格式的 skill 文件。
async fn load_skill_json(registry: &mut SkillRegistry, path: &Path) {
    match tokio::fs::read_to_string(path).await {
        Ok(raw) => match serde_json::from_str::<SkillFile>(&raw) {
            Ok(sf) => {
                registry.register(Skill {
                    name:        sf.name,
                    description: sf.description,
                    content:     sf.prompt,
                    tools:       sf.tools,
                    source:      SkillSource::Plugin,
                    location:    None,
                    slash:       true,
                });
            }
            Err(e) => warn!(file = %path.display(), err = %e, "failed to parse skill JSON"),
        },
        Err(e) => warn!(file = %path.display(), err = %e, "failed to read skill JSON"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skill_md_with_frontmatter() {
        let raw = "---\nname: my-skill\ndescription: A test skill\nslash: true\n---\n# My Skill\n\nBody text.";
        let (fm, body) = parse_skill_md(raw);
        assert_eq!(fm.name.as_deref(), Some("my-skill"));
        assert_eq!(fm.description.as_deref(), Some("A test skill"));
        assert!(fm.slash);
        assert!(body.contains("# My Skill"));
        assert!(body.contains("Body text."));
    }

    #[test]
    fn parse_skill_md_without_frontmatter() {
        let raw = "# Just Markdown\n\nNo frontmatter here.";
        let (fm, body) = parse_skill_md(raw);
        assert!(fm.name.is_none());
        assert!(fm.description.is_none());
        assert!(!fm.slash);
        assert!(body.contains("Just Markdown"));
    }

    #[test]
    fn parse_skill_md_quoted_values() {
        let raw = "---\nname: \"quoted-name\"\ndescription: 'single quoted'\n---\nBody";
        let (fm, _) = parse_skill_md(raw);
        assert_eq!(fm.name.as_deref(), Some("quoted-name"));
        assert_eq!(fm.description.as_deref(), Some("single quoted"));
    }

    #[test]
    fn parse_skill_md_no_slash_defaults_false() {
        let raw = "---\nname: test\ndescription: test\n---\nBody";
        let (fm, _) = parse_skill_md(raw);
        assert!(!fm.slash);
    }

    // ── 参数替换测试 ────────────────────────────────────────────────────────

    #[test]
    fn substitute_positional() {
        let args = vec!["hello".into(), "world".into()];
        assert_eq!(substitute_args("Say $1 and $2", &args), "Say hello and world");
    }

    #[test]
    fn substitute_all_args() {
        let args = vec!["foo".into(), "bar".into(), "baz".into()];
        assert_eq!(substitute_args("Run $@", &args), "Run foo bar baz");
        assert_eq!(substitute_args("Run $ARGUMENTS", &args), "Run foo bar baz");
    }

    #[test]
    fn substitute_default() {
        let args = vec!["hello".into()];
        assert_eq!(substitute_args("${2:-fallback}", &args), "fallback");
        let args2 = vec!["a".into(), "b".into()];
        assert_eq!(substitute_args("${2:-fallback}", &args2), "b");
    }

    #[test]
    fn substitute_slice() {
        let args = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        assert_eq!(substitute_args("${@:2}", &args), "b c d");
        assert_eq!(substitute_args("${@:2:2}", &args), "b c");
    }

    #[test]
    fn substitute_all_default() {
        let args: Vec<String> = vec![];
        assert_eq!(substitute_args("${@:-nothing}", &args), "nothing");
    }

    #[test]
    fn substitute_missing_keeps_placeholder() {
        let args: Vec<String> = vec![];
        assert_eq!(substitute_args("$1 is missing", &args), "$1 is missing");
    }

    #[test]
    fn parse_command_args_basic() {
        assert_eq!(parse_command_args("hello world"), vec!["hello", "world"]);
        assert_eq!(parse_command_args("  foo  bar  "), vec!["foo", "bar"]);
        assert_eq!(parse_command_args(""), Vec::<String>::new());
    }

    #[test]
    fn parse_command_args_quoted() {
        assert_eq!(
            parse_command_args(r#"foo "bar baz" qux"#),
            vec!["foo", "bar baz", "qux"]
        );
        assert_eq!(
            parse_command_args("'single' \"double\""),
            vec!["single", "double"]
        );
    }
}
