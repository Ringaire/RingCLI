// 输入历史持久化：~/.local/state/ring/history

use std::collections::VecDeque;
use tokio::io::AsyncWriteExt;

const MAX_HISTORY: usize = 1000;

pub struct History {
    entries: VecDeque<String>,
    /// 浏览游标：None 表示在最新（编辑行），Some(i) 表示正在浏览第 i 条
    cursor:  Option<usize>,
}

impl History {
    /// 从磁盘加载历史。
    pub async fn load() -> Self {
        let path = ring_core::session::paths::history_path();
        let entries = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => raw
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect::<VecDeque<_>>(),
            Err(_) => VecDeque::new(),
        };
        Self { entries, cursor: None }
    }

    /// 追加一条历史（去重连续相同），并落盘。
    pub async fn push(&mut self, entry: &str) {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.entries.back().map(|s| s.as_str()) == Some(trimmed) {
            self.cursor = None;
            return;
        }
        self.entries.push_back(trimmed.to_string());
        while self.entries.len() > MAX_HISTORY {
            self.entries.pop_front();
        }
        self.cursor = None;
        self.persist().await;
    }

    async fn persist(&self) {
        let path = ring_core::session::paths::history_path();
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let mut content = String::new();
        for e in &self.entries {
            content.push_str(e);
            content.push('\n');
        }
        if let Ok(mut f) = tokio::fs::File::create(&path).await {
            let _ = f.write_all(content.as_bytes()).await;
        }
    }

    /// 向上浏览（更旧），返回该条内容。
    pub fn prev(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let new_cursor = match self.cursor {
            None    => self.entries.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(new_cursor);
        self.entries.get(new_cursor).cloned()
    }

    /// 向下浏览（更新），返回该条内容；越过最新返回空串。
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<String> {
        match self.cursor {
            None => None,
            Some(i) if i + 1 >= self.entries.len() => {
                self.cursor = None;
                Some(String::new())
            }
            Some(i) => {
                self.cursor = Some(i + 1);
                self.entries.get(i + 1).cloned()
            }
        }
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = None;
    }
}
