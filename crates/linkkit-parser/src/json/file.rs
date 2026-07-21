//! 文件操作 JSON 命令

use serde::{Deserialize, Serialize};
use crate::error::{LinkkitError, LinkkitResult};
use crate::tags::LinkkitTag;
use std::path::PathBuf;

// ─── Ls 命令 ────────────────────────────────────────────────────────────

/// 文件列表命令
///
/// # 示例
///
/// ```json
/// {"ls": "/home/Downloads"}
/// {"ls": {"dir": ["/home/Downloads", "/home/Music"], "tail": "50"}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsCommand {
    /// 目录路径或复杂参数
    pub ls: LsTarget,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

/// Ls 目标：字符串路径或对象
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LsTarget {
    /// 单个路径字符串
    Path(String),
    /// 复杂参数对象
    Complex {
        dir: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tail: Option<String>,
    },
}

impl LsCommand {
    /// 转换为 LinkkitTag
    ///
    /// 注意：当前 LinkkitTag 没有专门的 Ls 标签，使用 Bash 包装
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        let command = match self.ls {
            LsTarget::Path(path) => format!("ls -la {}", shell_escape(&path)),
            LsTarget::Complex { dir, tail } => {
                let paths = dir.iter()
                    .map(|p| shell_escape(p))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut cmd = format!("ls -la {}", paths);
                if let Some(n) = tail {
                    cmd = format!("{} | tail -n {}", cmd, n);
                }
                cmd
            }
        };

        Ok(LinkkitTag::Bash {
            command,
            timeout: None,
            tail: None,
            bg: false,
            at: None,
        })
    }
}

// ─── Tree 命令 ──────────────────────────────────────────────────────────

/// 文件树命令
///
/// # 示例
///
/// ```json
/// {"tree": "/home/Downloads"}
/// {"tree": {"dir": ["/home/Downloads", "/home/Music"]}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeCommand {
    /// 目录路径或复杂参数
    pub tree: TreeTarget,

    /// 递归深度
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<usize>,

    /// 排除模式
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,

    /// 显示全部（绕过过滤）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

/// Tree 目标：字符串路径或对象
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TreeTarget {
    /// 单个路径字符串
    Path(String),
    /// 复杂参数对象
    Complex { dir: Vec<String> },
}

impl TreeCommand {
    /// 转换为 LinkkitTag::Tree
    ///
    /// 注意：仅支持单个路径，多路径取第一个
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        let path = match self.tree {
            TreeTarget::Path(p) => p,
            TreeTarget::Complex { mut dir } => {
                if dir.is_empty() {
                    return Err(LinkkitError::Other("tree 命令需要至少一个目录".to_string()));
                }
                dir.remove(0)
            }
        };

        Ok(LinkkitTag::Tree {
            path: PathBuf::from(path),
            level: self.level,
            exclude: self.exclude,
            all: self.all.unwrap_or(false),
        })
    }
}

// ─── Grep 命令 ──────────────────────────────────────────────────────────

/// 全局正则搜索命令
///
/// # 示例
///
/// ```json
/// {"grep": "/home", "pattern": "TODO"}
/// {"grep": {"dir": ["/home/src"], "pattern": "TODO"}}
/// {"grep": {"file": "install.sh", "pattern": "TODO"}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepCommand {
    /// 搜索目标
    pub grep: GrepTarget,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

/// Grep 目标
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GrepTarget {
    /// 简单路径字符串（需配合外部 pattern）
    Path(String),
    /// 目录搜索
    Dir {
        dir: Vec<String>,
        pattern: String,
    },
    /// 文件搜索
    File {
        file: String,
        pattern: String,
    },
}

impl GrepCommand {
    /// 转换为 LinkkitTag
    ///
    /// 使用 Bash 包装 grep/rg 命令
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        let command = match self.grep {
            GrepTarget::Path(_path) => {
                return Err(LinkkitError::Other(
                    "grep 简单路径格式需要 pattern 参数".to_string()
                ));
            }
            GrepTarget::Dir { dir, pattern } => {
                let paths = dir.iter()
                    .map(|p| shell_escape(p))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("rg {} {}", shell_escape(&pattern), paths)
            }
            GrepTarget::File { file, pattern } => {
                format!("rg {} {}", shell_escape(&pattern), shell_escape(&file))
            }
        };

        Ok(LinkkitTag::Bash {
            command,
            timeout: None,
            tail: None,
            bg: false,
            at: None,
        })
    }
}

// ─── Find 命令 ──────────────────────────────────────────────────────────

/// 查找文件命令
///
/// # 示例
///
/// ```json
/// {"find": "*.txt"}
/// {"find": "*.rs", "say": "查找 Rust 文件"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindCommand {
    /// 文件模式（支持通配符）
    pub find: String,

    /// 搜索目录
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,

    /// 操作描述（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
}

impl FindCommand {
    /// 转换为 LinkkitTag
    ///
    /// 使用 Bash 包装 find 命令
    pub fn into_tag(self) -> LinkkitResult<LinkkitTag> {
        let dir = self.dir.as_deref().unwrap_or(".");
        let command = format!(
            "find {} -name {}",
            shell_escape(dir),
            shell_escape(&self.find)
        );

        Ok(LinkkitTag::Bash {
            command,
            timeout: None,
            tail: None,
            bg: false,
            at: None,
        })
    }
}

// ─── 辅助函数 ───────────────────────────────────────────────────────────

/// Shell 字符串转义（简单实现）
fn shell_escape(s: &str) -> String {
    if s.contains(' ') || s.contains('$') || s.contains('*') || s.contains('?') || s.contains('\'') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ls_simple() {
        let json = r#"{"ls": "/home/Downloads"}"#;
        let cmd: LsCommand = serde_json::from_str(json).unwrap();
        
        match cmd.ls {
            LsTarget::Path(path) => assert_eq!(path, "/home/Downloads"),
            _ => panic!("Expected Path variant"),
        }
    }

    #[test]
    fn test_ls_complex() {
        let json = r#"{"ls": {"dir": ["/home", "/tmp"], "tail": "50"}}"#;
        let cmd: LsCommand = serde_json::from_str(json).unwrap();
        
        match cmd.ls {
            LsTarget::Complex { dir, tail } => {
                assert_eq!(dir.len(), 2);
                assert_eq!(tail, Some("50".to_string()));
            }
            _ => panic!("Expected Complex variant"),
        }
    }

    #[test]
    fn test_tree_simple() {
        let json = r#"{"tree": "/home/src"}"#;
        let cmd: TreeCommand = serde_json::from_str(json).unwrap();
        
        match cmd.tree {
            TreeTarget::Path(path) => assert_eq!(path, "/home/src"),
            _ => panic!("Expected Path variant"),
        }
    }

    #[test]
    fn test_grep_dir() {
        let json = r#"{"grep": {"dir": ["/home/src"], "pattern": "TODO"}}"#;
        let cmd: GrepCommand = serde_json::from_str(json).unwrap();
        
        match cmd.grep {
            GrepTarget::Dir { dir, pattern } => {
                assert_eq!(dir.len(), 1);
                assert_eq!(pattern, "TODO");
            }
            _ => panic!("Expected Dir variant"),
        }
    }

    #[test]
    fn test_find_simple() {
        let json = r#"{"find": "*.txt"}"#;
        let cmd: FindCommand = serde_json::from_str(json).unwrap();
        
        assert_eq!(cmd.find, "*.txt");
        assert_eq!(cmd.dir, None);
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
    }
}
