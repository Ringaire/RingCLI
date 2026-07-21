use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::GateError;

/// Linkkit 权限闸门
///
/// 实现两道闸：
/// - read-gate: 修改文件前必须先读取
/// - doc-gate: 调用工具前必须先读取文档
#[derive(Debug, Clone, Default)]
pub struct LinkkitGate {
    /// 已读取的文件路径集合
    read_files: HashSet<PathBuf>,
    /// 已读取文档的工具名称集合
    doc_read_tools: HashSet<String>,
}

impl LinkkitGate {
    pub fn new() -> Self {
        Self::default()
    }

    // ─── read-gate ──────────────────────────────────────────────────────────

    /// 检查 edit-gate：修改文件前必须先读取
    ///
    /// 文件不存在时放行（允许新建文件）
    pub fn check_edit_gate(&self, file: &Path) -> Result<(), GateError> {
        if !self.read_files.contains(file) {
            return Err(GateError::MustReadFirst(file.to_path_buf()));
        }
        Ok(())
    }

    /// 标记文件已读取
    pub fn mark_read(&mut self, file: PathBuf) {
        self.read_files.insert(file);
    }

    /// 检查文件是否已读取
    pub fn has_read(&self, file: &Path) -> bool {
        self.read_files.contains(file)
    }

    /// 清除已读文件记录（用于会话重置）
    pub fn clear_read_files(&mut self) {
        self.read_files.clear();
    }

    // ─── doc-gate ───────────────────────────────────────────────────────────

    /// 检查 doc-gate：调用工具前必须先读取文档
    pub fn check_doc_gate(&self, tool: &str) -> Result<(), GateError> {
        if !self.doc_read_tools.contains(tool) {
            return Err(GateError::MustReadDocFirst(tool.to_string()));
        }
        Ok(())
    }

    /// 标记工具文档已读取
    pub fn mark_doc_read(&mut self, tool: String) {
        self.doc_read_tools.insert(tool);
    }

    /// 检查工具文档是否已读取
    pub fn has_doc_read(&self, tool: &str) -> bool {
        self.doc_read_tools.contains(tool)
    }

    /// 清除已读工具文档记录（用于会话重置）
    pub fn clear_doc_reads(&mut self) {
        self.doc_read_tools.clear();
    }

    /// 完全重置闸门状态
    pub fn reset(&mut self) {
        self.read_files.clear();
        self.doc_read_tools.clear();
    }

    // ─── 统计与调试 ─────────────────────────────────────────────────────────

    /// 获取已读文件数量
    pub fn read_files_count(&self) -> usize {
        self.read_files.len()
    }

    /// 获取已读工具文档数量
    pub fn doc_reads_count(&self) -> usize {
        self.doc_read_tools.len()
    }

    /// 获取所有已读文件的引用
    pub fn read_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.read_files.iter()
    }

    /// 获取所有已读工具文档的引用
    pub fn doc_read_tools(&self) -> impl Iterator<Item = &str> {
        self.doc_read_tools.iter().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_gate() {
        let mut gate = LinkkitGate::new();
        let file = PathBuf::from("test.txt");

        // 未读取时拒绝
        assert!(gate.check_edit_gate(&file).is_err());

        // 标记已读取
        gate.mark_read(file.clone());
        assert!(gate.check_edit_gate(&file).is_ok());
        assert!(gate.has_read(&file));

        // 清除后再次拒绝
        gate.clear_read_files();
        assert!(gate.check_edit_gate(&file).is_err());
    }

    #[test]
    fn test_doc_gate() {
        let mut gate = LinkkitGate::new();
        let tool = "web_fetch";

        // 未读取时拒绝
        assert!(gate.check_doc_gate(tool).is_err());

        // 标记已读取
        gate.mark_doc_read(tool.to_string());
        assert!(gate.check_doc_gate(tool).is_ok());
        assert!(gate.has_doc_read(tool));

        // 清除后再次拒绝
        gate.clear_doc_reads();
        assert!(gate.check_doc_gate(tool).is_err());
    }

    #[test]
    fn test_reset() {
        let mut gate = LinkkitGate::new();
        gate.mark_read(PathBuf::from("test.txt"));
        gate.mark_doc_read("tool".to_string());

        assert_eq!(gate.read_files_count(), 1);
        assert_eq!(gate.doc_reads_count(), 1);

        gate.reset();
        assert_eq!(gate.read_files_count(), 0);
        assert_eq!(gate.doc_reads_count(), 0);
    }
}
