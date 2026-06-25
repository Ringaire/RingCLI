use std::collections::HashMap;
use std::path::PathBuf;

// ── Skill trait（定义在 core，实现在 neko-skills）────────────────────────────

#[derive(Debug, Clone)]
pub enum SkillSource {
    Builtin,
    Mcp,
    /// 从 .agents/skills/、.neko/skills/、~/.config/neko/skills/ 等 目录加载
    Filesystem,
    /// 旧 JSON 格式
    Plugin,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name:        String,
    pub description: String,
    /// SKILL.md 正文内容（Markdown）或旧 JSON 格式的 prompt
    pub content:     String,
    /// 旧 JSON 格式的 tools 字段（可选，暂未用于权限控制）
    pub tools:       Vec<String>,
    pub source:      SkillSource,
    /// SKILL.md 文件的目录路径（用于扫描辅助文件，如脚本、参考文档）
    pub location:    Option<PathBuf>,
    /// 是否注册为 slash 命令（SKILL.md frontmatter `slash: true`）
    pub slash:       bool,
}

impl Skill {
    /// 兼容旧字段名 `prompt` 的访问器。
    pub fn prompt(&self) -> &str {
        &self.content
    }
}

// ── SkillRegistry ─────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    pub fn unregister(&mut self, name: &str) {
        self.skills.remove(name);
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<&Skill> {
        let mut v: Vec<&Skill> = self.skills.values().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// 旧的 listing 格式（slash 命令列表用）。
    pub fn build_listing(&self) -> String {
        let mut out = String::from("## Available Skills\n");
        for s in self.list() {
            out.push_str(&format!("- /{}: {}\n", s.name, s.description));
        }
        out
    }

    /// 构建 `<available_skills>` XML 块，注入 system prompt。
    /// AI 看到匹配任务时自动调用 `skill` 工具加载内容。
    pub fn build_available_skills(&self) -> String {
        let list = self.list();
        if list.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "Skills provide specialized instructions and workflows for specific tasks.\n\
             Use the skill tool to load a skill when a task matches its description.\n",
        );
        out.push_str("<available_skills>\n");
        for s in &list {
            out.push_str("  <skill>\n");
            out.push_str(&format!("    <name>{}</name>\n", s.name));
            out.push_str(&format!("    <description>{}</description>\n", s.description));
            out.push_str("  </skill>\n");
        }
        out.push_str("</available_skills>");
        out
    }

    /// 获取指定 skill 的内容（slash 命令调用时展开）。
    pub fn build_content(&self, name: &str) -> Option<String> {
        self.skills.get(name).map(|s| {
            format!("## Skill: {}\n\n{}\n", s.name, s.content)
        })
    }
}
