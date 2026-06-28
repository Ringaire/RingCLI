use neko_core::skills::{Skill, SkillRegistry, SkillSource};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::warn;

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
}
