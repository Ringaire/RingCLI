use crate::error::{GateError, LinkkitError, LinkkitResult};
use crate::gate::LinkkitGate;
use crate::tags::{LinkkitTag, ToolArgs};

/// Linkkit 执行器
///
/// 负责执行解析后的 Linkkit 标签，集成权限闸门检查
pub struct LinkkitExecutor {
    gate: LinkkitGate,
}

impl LinkkitExecutor {
    pub fn new() -> Self {
        Self {
            gate: LinkkitGate::new(),
        }
    }

    pub fn with_gate(gate: LinkkitGate) -> Self {
        Self { gate }
    }

    pub fn gate(&self) -> &LinkkitGate {
        &self.gate
    }

    pub fn gate_mut(&mut self) -> &mut LinkkitGate {
        &mut self.gate
    }

    /// 执行单个标签
    ///
    /// 注意：此方法仅处理闸门检查，实际的工具调用需要由上层引擎完成
    pub async fn execute(&mut self, tag: LinkkitTag) -> LinkkitResult<ExecutionResult> {
        match tag {
            // ─── 文档管理 ───────────────────────────────────────────────────
            LinkkitTag::DocLs => Ok(ExecutionResult::DocLs),

            LinkkitTag::DocRead { name, line } => {
                // 标记工具文档已读（doc-gate）
                if let Some(ref tool_name) = name {
                    self.gate.mark_doc_read(tool_name.clone());
                }
                Ok(ExecutionResult::DocRead { name, line })
            }

            // ─── 工具管理 ───────────────────────────────────────────────────
            LinkkitTag::ToolLs { profile } => Ok(ExecutionResult::ToolLs { profile }),

            LinkkitTag::ToolInfo { name } => Ok(ExecutionResult::ToolInfo { name }),

            LinkkitTag::ToolUse { name, args } => {
                // 检查 doc-gate
                self.gate.check_doc_gate(&name).map_err(|e| match e {
                    GateError::MustReadDocFirst(tool) => LinkkitError::Other(format!(
                        "doc-gate: 必须先读取工具文档才能调用。使用: <doc-read name=\"{}\"/>",
                        tool
                    )),
                    _ => LinkkitError::Other(e.to_string()),
                })?;

                Ok(ExecutionResult::ToolUse { name, args })
            }

            LinkkitTag::ToolReload => Ok(ExecutionResult::ToolReload),

            // ─── 命令执行 ───────────────────────────────────────────────────
            LinkkitTag::Bash {
                command,
                timeout,
                tail,
                bg,
                at,
            } => Ok(ExecutionResult::Bash {
                command,
                timeout,
                tail,
                bg,
                at,
            }),

            LinkkitTag::BashLs { all, find } => Ok(ExecutionResult::BashLs { all, find }),

            LinkkitTag::BashKill { task_id } => Ok(ExecutionResult::BashKill { task_id }),

            LinkkitTag::BashLog { task_id, line, tail } => {
                Ok(ExecutionResult::BashLog { task_id, line, tail })
            }

            // ─── 文件操作 ───────────────────────────────────────────────────
            LinkkitTag::Read { file, line, tail } => {
                // 标记文件已读（read-gate）
                self.gate.mark_read(file.clone());
                Ok(ExecutionResult::Read { file, line, tail })
            }

            LinkkitTag::Edit {
                file,
                old,
                new,
                all,
                content,
            } => {
                // 检查 read-gate（仅当文件已存在时）
                // 注意：实际的文件存在性检查由上层工具实现
                // 这里假设如果是新建文件（content 模式），则不需要检查
                if old.is_some() || new.is_some() {
                    self.gate.check_edit_gate(&file).map_err(|e| match e {
                        GateError::MustReadFirst(path) => LinkkitError::Other(format!(
                            "read-gate: 必须先读取文件才能编辑。使用: <read file=\"{}\"/>",
                            path.display()
                        )),
                        _ => LinkkitError::Other(e.to_string()),
                    })?;
                }

                Ok(ExecutionResult::Edit {
                    file,
                    old,
                    new,
                    all,
                    content,
                })
            }

            LinkkitTag::Write { file, content } => {
                // Write 用于新建文件，不需要 read-gate
                Ok(ExecutionResult::Write { file, content })
            }

            // ─── 目录浏览 ───────────────────────────────────────────────────
            LinkkitTag::Tree {
                path,
                level,
                exclude,
                all,
            } => Ok(ExecutionResult::Tree {
                path,
                level,
                exclude,
                all,
            }),

            // ─── 网页抓取 ───────────────────────────────────────────────────
            LinkkitTag::WebFetch { url } => Ok(ExecutionResult::WebFetch { url }),

            // ─── TODO 管理 ──────────────────────────────────────────────────
            LinkkitTag::TodoUpdate { content } => Ok(ExecutionResult::TodoUpdate { content }),

            LinkkitTag::TodoDone => Ok(ExecutionResult::TodoDone),

            LinkkitTag::TodoClear => Ok(ExecutionResult::TodoClear),

            // ─── 子 Agent ───────────────────────────────────────────────────
            LinkkitTag::SubAgent { prompt, name, mode } => {
                Ok(ExecutionResult::SubAgent { prompt, name, mode })
            }

            LinkkitTag::SubTask { all, find } => Ok(ExecutionResult::SubTask { all, find }),

            LinkkitTag::SubCancel { task_id } => Ok(ExecutionResult::SubCancel { task_id }),

            // ─── 事件订阅 ───────────────────────────────────────────────────
            LinkkitTag::Event {
                form,
                name,
                task,
                pid,
                time,
                clock,
                day,
                everytime,
                path,
                shell,
                max,
            } => Ok(ExecutionResult::Event {
                form,
                name,
                task,
                pid,
                time,
                clock,
                day,
                everytime,
                path,
                shell,
                max,
            }),

            LinkkitTag::EventLs { all, find } => Ok(ExecutionResult::EventLs { all, find }),

            LinkkitTag::EventCancel { id, form } => Ok(ExecutionResult::EventCancel { id, form }),

            // ─── 询问用户 ───────────────────────────────────────────────────
            LinkkitTag::Ask { question, options } => Ok(ExecutionResult::Ask { question, options }),
        }
    }

    /// 批量执行标签
    pub async fn execute_batch(&mut self, tags: Vec<LinkkitTag>) -> Vec<LinkkitResult<ExecutionResult>> {
        let mut results = Vec::with_capacity(tags.len());
        for tag in tags {
            results.push(self.execute(tag).await);
        }
        results
    }

    /// 重置执行器状态（清除闸门记录）
    pub fn reset(&mut self) {
        self.gate.reset();
    }
}

impl Default for LinkkitExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// 执行结果
///
/// 包含解析后的命令信息，等待上层引擎实际执行
#[derive(Debug, Clone)]
pub enum ExecutionResult {
    // 文档管理
    DocLs,
    DocRead {
        name: Option<String>,
        line: Option<String>,
    },

    // 工具管理
    ToolLs {
        profile: bool,
    },
    ToolInfo {
        name: String,
    },
    ToolUse {
        name: String,
        args: ToolArgs,
    },
    ToolReload,

    // 命令执行
    Bash {
        command: String,
        timeout: Option<u64>,
        tail: Option<usize>,
        bg: bool,
        at: Option<std::path::PathBuf>,
    },
    BashLs {
        all: bool,
        find: Option<String>,
    },
    BashKill {
        task_id: String,
    },
    BashLog {
        task_id: String,
        line: Option<String>,
        tail: Option<usize>,
    },

    // 文件操作
    Read {
        file: std::path::PathBuf,
        line: Option<String>,
        tail: Option<usize>,
    },
    Edit {
        file: std::path::PathBuf,
        old: Option<String>,
        new: Option<String>,
        all: bool,
        content: Option<String>,
    },
    Write {
        file: std::path::PathBuf,
        content: String,
    },

    // 目录浏览
    Tree {
        path: std::path::PathBuf,
        level: Option<usize>,
        exclude: Option<String>,
        all: bool,
    },

    // 网页抓取
    WebFetch {
        url: String,
    },

    // TODO 管理
    TodoUpdate {
        content: String,
    },
    TodoDone,
    TodoClear,

    // 子 Agent
    SubAgent {
        prompt: String,
        name: Option<String>,
        mode: Option<String>,
    },
    SubTask {
        all: bool,
        find: Option<String>,
    },
    SubCancel {
        task_id: String,
    },

    // 事件订阅
    Event {
        form: String,
        name: Option<String>,
        task: Option<String>,
        pid: Option<u32>,
        time: Option<String>,
        clock: Option<String>,
        day: Option<String>,
        everytime: Option<String>,
        path: Option<std::path::PathBuf>,
        shell: Option<String>,
        max: Option<usize>,
    },
    EventLs {
        all: bool,
        find: Option<String>,
    },
    EventCancel {
        id: Option<String>,
        form: Option<String>,
    },

    // 询问用户
    Ask {
        question: String,
        options: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_doc_gate() {
        let mut executor = LinkkitExecutor::new();

        // 未读取文档时拒绝
        let result = executor
            .execute(LinkkitTag::ToolUse {
                name: "web_fetch".to_string(),
                args: ToolArgs::Single("https://example.com".to_string()),
            })
            .await;
        assert!(result.is_err());

        // 读取文档后放行
        executor
            .execute(LinkkitTag::DocRead {
                name: Some("web_fetch".to_string()),
                line: None,
            })
            .await
            .unwrap();

        let result = executor
            .execute(LinkkitTag::ToolUse {
                name: "web_fetch".to_string(),
                args: ToolArgs::Single("https://example.com".to_string()),
            })
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_read_gate() {
        let mut executor = LinkkitExecutor::new();
        let file = PathBuf::from("test.txt");

        // 未读取文件时拒绝编辑
        let result = executor
            .execute(LinkkitTag::Edit {
                file: file.clone(),
                old: Some("old".to_string()),
                new: Some("new".to_string()),
                all: false,
                content: None,
            })
            .await;
        assert!(result.is_err());

        // 读取文件后放行
        executor
            .execute(LinkkitTag::Read {
                file: file.clone(),
                line: None,
                tail: None,
            })
            .await
            .unwrap();

        let result = executor
            .execute(LinkkitTag::Edit {
                file: file.clone(),
                old: Some("old".to_string()),
                new: Some("new".to_string()),
                all: false,
                content: None,
            })
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_write_no_gate() {
        let mut executor = LinkkitExecutor::new();
        let file = PathBuf::from("new.txt");

        // Write 不需要 read-gate（用于新建文件）
        let result = executor
            .execute(LinkkitTag::Write {
                file,
                content: "content".to_string(),
            })
            .await;
        assert!(result.is_ok());
    }
}
