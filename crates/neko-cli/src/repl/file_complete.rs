//! `@file` 路径补全：输入 `@` 时在 cwd 下列出文件，复用现有命令补全下拉。
//!
//! 设计对照 Claude Code（`hooks/useTypeahead.tsx`）：
//! - `@` 仅在**行首或前面是空白**时触发（`email@host` 不误触发）。
//! - token = `@` + 非空白路径串（到光标处）。
//! - 候选的 `value` 是**整行替换后的输入**（`@partial` → `@fullpath `），
//!   这样 app.rs 的接受逻辑（`input.set(value)`）无需改动即可复用。
//!
//! 文件索引按 cwd 懒构建一次并缓存（`ignore` 走 `.gitignore`、跳 `.git`），后续按 query 内存过滤。

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ignore::WalkBuilder;

use crate::repl::commands::Suggestion;

/// 单次下拉最多展示的文件数。
const MAX_RESULTS: usize = 12;
/// 索引遍历的文件数上限（防超大仓库卡顿）。
const MAX_INDEX: usize = 20_000;
/// 遍历最大深度。
const MAX_DEPTH: usize = 12;

/// 索引陈旧多久后重建（对照 CC 的 5s 兜底刷新；输入连打期间走缓存）。
const REFRESH_AFTER: Duration = Duration::from_secs(3);

/// 缓存的 cwd 文件相对路径列表 + 上次构建时刻（`None` = 从未构建，视为陈旧）。
/// 用 `Option` 哨兵而非 `Instant - Duration`：后者在开机数秒内会因单调时钟下溢 panic。
static INDEX: OnceLock<Mutex<Option<(Instant, Arc<Vec<String>>)>>> = OnceLock::new();

/// 检测光标处的 `@token`：返回 `(token 起始字节, query)`。
/// `@` 须在行首或紧跟空白；token 内不含空白。
pub fn at_token(input: &str, cursor: usize) -> Option<(usize, String)> {
    let end = cursor.min(input.len());
    let before = &input[..end];
    let at = before.rfind('@')?;
    if at > 0 {
        // `@` 前一个字符必须是空白
        let prev = before[..at].chars().next_back();
        if !prev.map(|c| c.is_whitespace()).unwrap_or(false) {
            return None;
        }
    }
    let query = &before[at + 1..];
    if query.contains(char::is_whitespace) {
        return None;
    }
    Some((at, query.to_string()))
}

// ── fzf 式模糊打分（对照 CC `native-ts/file-index` 的 search 评分）──────────────
const SCORE_MATCH: i32 = 16;
const BONUS_BOUNDARY: i32 = 8;   // 匹配点前是 / \ - _ . space
const BONUS_CAMEL: i32 = 7;      // 小写后接大写
const BONUS_CONSEC: i32 = 4;     // 与上一匹配字符相邻
const BONUS_FIRST: i32 = 8;      // 匹配在路径首字符
const PENALTY_GAP_START: i32 = -3;
const PENALTY_GAP_EXT: i32 = -1;

fn is_boundary(c: char) -> bool {
    matches!(c, '/' | '\\' | '-' | '_' | '.' | ' ')
}

/// `query` 为 `path` 的（贪心最早）子序列则返回模糊分（越大越好），否则 `None`。
/// smart-case：query 含大写 → 区分大小写，否则不区分。
fn fuzzy_score(path: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let case_sensitive = query.chars().any(|c| c.is_uppercase());
    let eq = |a: char, b: char| {
        if case_sensitive { a == b } else { a.eq_ignore_ascii_case(&b) }
    };
    let hay: Vec<char> = path.chars().collect();
    let need: Vec<char> = query.chars().collect();

    let mut score = 0i32;
    let mut ni = 0usize;
    let mut prev: Option<usize> = None;
    for (i, &hc) in hay.iter().enumerate() {
        if ni >= need.len() {
            break;
        }
        if eq(hc, need[ni]) {
            score += SCORE_MATCH;
            if i == 0 {
                score += BONUS_FIRST;
            } else {
                let p = hay[i - 1];
                if is_boundary(p) {
                    score += BONUS_BOUNDARY;
                } else if p.is_lowercase() && hc.is_uppercase() {
                    score += BONUS_CAMEL;
                }
            }
            if let Some(pi) = prev {
                let gap = i - pi - 1;
                if gap == 0 {
                    score += BONUS_CONSEC;
                } else {
                    score += PENALTY_GAP_START + gap as i32 * PENALTY_GAP_EXT;
                }
            }
            prev = Some(i);
            ni += 1;
        }
    }
    if ni < need.len() {
        return None; // 不是子序列
    }
    // 短路径轻微加分（对照 CC 的 32 - len>>2）。
    score += (32 - (hay.len() as i32 >> 2)).max(0);
    Some(score)
}

/// 若光标处存在 `@token` 则返回文件补全候选（可能为空），否则返回 `None`（交给命令补全）。
pub fn maybe_suggestions(input: &str, cursor: usize, cwd: &Path) -> Option<Vec<Suggestion>> {
    let (at, query) = at_token(input, cursor)?;
    let end = cursor.min(input.len());
    let prefix = &input[..at];
    let suffix = input[end..].to_string();

    // 空 query：按层级浅、路径短优先取前 N（对照 CC 的 topLevelCache）。
    // 非空：fzf 式模糊打分排序（对照 CC FileIndex.search）。
    let idx = index(cwd);
    let hits: Vec<&str> = if query.is_empty() {
        let mut v: Vec<&str> = idx.iter().map(String::as_str).collect();
        v.sort_by_key(|p| (p.matches('/').count(), p.len()));
        v.truncate(MAX_RESULTS);
        v
    } else {
        let mut scored: Vec<(i32, &str)> = idx
            .iter()
            .filter_map(|p| fuzzy_score(p, &query).map(|s| (s, p.as_str())))
            .collect();
        // 分高优先；同分短路径、字典序。
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())).then(a.1.cmp(b.1)));
        scored.truncate(MAX_RESULTS);
        scored.into_iter().map(|(_, p)| p).collect()
    };

    let out = hits
        .into_iter()
        .map(|path| {
            let mut value = format!("{prefix}@{path}");
            if suffix.is_empty() {
                value.push(' ');
            } else {
                value.push(' ');
                value.push_str(suffix.trim_start());
            }
            Suggestion {
                value,
                label: format!("@{path}"),
                description: String::new(),
            }
        })
        .collect();
    Some(out)
}

/// 单个 `@file` 注入的大小上限；超出则截断并标注（对照 CC 的 `truncated` 处理）。
const MAX_FILE_BYTES: usize = 128 * 1024;

/// 提交时展开 `@path`：读取 cwd 内**真实文件**的内容，追加到**发给模型的消息**（对照 CC 的
/// `FileAttachment{ content }` —— @ 提及即注入内容，而非只给引用让模型再去读）。
///
/// - 显示给用户的原文保持不变（`@path` token）；只有模型消息追加这里的内容段。
/// - 安全：只读 cwd 范围内的文件（canonicalize + starts_with）；越界 / 非 UTF-8 / 不存在 → 跳过。
/// - 大文件按 `MAX_FILE_BYTES` 截断并标注。无可解析引用时返回空串。
/// - （CC 的 readFileState 去重 / 压缩后引用 v1 先不做。）
pub fn expand_mentions(text: &str, cwd: &Path) -> String {
    use std::collections::HashSet;
    let cwd_canon = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = String::new();

    for tok in text.split_whitespace() {
        if tok.len() < 2 || !tok.starts_with('@') {
            continue;
        }
        let rel = &tok[1..];
        if !seen.insert(rel) {
            continue;
        }
        let Ok(canon) = cwd.join(rel).canonicalize() else { continue };
        if !canon.starts_with(&cwd_canon) || !canon.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&canon) else { continue }; // 非 UTF-8 跳过
        if content.len() > MAX_FILE_BYTES {
            let mut cut = MAX_FILE_BYTES;
            while !content.is_char_boundary(cut) { cut -= 1; }
            out.push_str(&format!(
                "\n\n[attached file: {rel} — truncated to first 128KB]\n```\n{}\n```",
                &content[..cut]
            ));
        } else {
            out.push_str(&format!("\n\n[attached file: {rel}]\n```\n{content}\n```"));
        }
    }
    out
}

// ── 粘贴处理：file:// URL → @path 引用 ─────────────────────────────────────

/// 对文件路径做百分号解码（`%20` → 空格 等）。
fn url_decode_path(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 把 `file://` URL 解析为本地路径。
///
/// 遵循 RFC 8089 `file:` URI 格式：`file://<host>/<path>`
///
/// | 场景 | 输入 | 输出 |
/// |------|------|------|
/// | Linux 本地 | `file:///home/user/f.txt` | `/home/user/f.txt` |
/// | localhost | `file://localhost/home/user/f.txt` | `/home/user/f.txt` |
/// | Windows 盘符 | `file:///C:/Users/f.txt` | `C:/Users/f.txt` |
/// | WSL（旧） | `file://wsl$/Ubuntu/home/u/f.txt` | `//wsl$/Ubuntu/home/u/f.txt` |
/// | WSL（新） | `file://wsl.localhost/Ubuntu/home/u/f.txt` | `//wsl.localhost/Ubuntu/home/u/f.txt` |
/// | SMB 网络路径 | `file://server/share/f.txt` | `//server/share/f.txt` |
///
/// 路径统一保持正斜杠 `/`——Rust `std::path::Path` 在 Windows 上同时接受
/// `/` 和 `\`，无需转换，避免反斜杠转义问题。
///
/// 非 `file://` 开头 → `None`。
fn file_url_to_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;

    // 分割 <host>/<path>：第一个 '/' 之前是 host，之后是 path。
    // rest 以 '/' 开头时 host 为空（`file:///path`）。
    let (host, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => return None, // 仅有 host 无 path，无效
    };

    let raw_path = if host.is_empty() || host == "localhost" {
        // 本地路径：Windows 盘符 /X:/... → X:/...
        let bytes = path.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
        {
            path[1..].to_string()
        } else {
            path.to_string()
        }
    } else {
        // 远程路径（WSL、SMB 等）→ UNC 路径 //host/path
        format!("//{host}{path}")
    };

    Some(url_decode_path(&raw_path))
}

/// 处理粘贴文本：把 `file://` URL 转换为 `@path` 引用。
///
/// - 整行（或多行）都是 `file://` URL → 每个转为 `@path`，空格分隔
/// - 文本中不含 `file://` → 原样返回
/// - 混合文本 → 原样返回（避免误伤普通文本）
///
/// 图片等二进制文件因 `read_to_string` 失败会被 `expand_mentions` 自动跳过，
/// 只保留路径文本让模型知晓文件位置。
pub fn process_paste(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return text.to_string();
    }
    let all_file_urls = lines.iter().all(|line| {
        let t = line.trim();
        !t.is_empty() && t.starts_with("file://")
    });
    if !all_file_urls {
        return text.to_string();
    }
    let paths: Vec<String> = lines.iter()
        .filter_map(|line| file_url_to_path(line.trim()))
        .map(|p| format!("@{p}"))
        .collect();
    if paths.is_empty() {
        text.to_string()
    } else {
        format!("{} ", paths.join(" "))
    }
}

/// 取 cwd 文件索引；陈旧（>`REFRESH_AFTER`）则重建（捡会话内新建的文件）。
fn index(cwd: &Path) -> Arc<Vec<String>> {
    let cell = INDEX.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let stale = match guard.as_ref() {
        Some((built, _)) => built.elapsed() >= REFRESH_AFTER,
        None => true,
    };
    if stale {
        *guard = Some((Instant::now(), Arc::new(build_index(cwd))));
    }
    // stale 分支已保证 Some。
    guard.as_ref().map(|(_, f)| f.clone()).unwrap_or_default()
}

fn build_index(cwd: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(cwd)
        .max_depth(Some(MAX_DEPTH))
        .git_ignore(true)
        .hidden(true)
        .build();
    for entry in walker.flatten() {
        if files.len() >= MAX_INDEX {
            break;
        }
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let rel = entry.path().strip_prefix(cwd).unwrap_or(entry.path());
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_token_triggers_at_start_and_after_space() {
        assert_eq!(at_token("@src", 4), Some((0, "src".into())));
        assert_eq!(at_token("hi @lib", 7), Some((3, "lib".into())));
    }

    #[test]
    fn no_trigger_when_at_glued_to_text() {
        assert_eq!(at_token("mail@host", 9), None); // email-like：@ 前非空白
    }

    #[test]
    fn no_trigger_with_space_in_token() {
        assert_eq!(at_token("@src main", 9), None);
    }

    #[test]
    fn token_uses_text_before_cursor() {
        // 光标在 "@sr" 后
        assert_eq!(at_token("@srcXYZ", 3), Some((0, "sr".into())));
    }

    #[test]
    fn fuzzy_matches_subsequence_and_ranks() {
        // 子序列匹配（substring 匹配不到的也能命中）
        assert!(fuzzy_score("src/main.rs", "mr").is_some());
        assert!(fuzzy_score("src/main.rs", "srcmain").is_some());
        // 非子序列 → None
        assert!(fuzzy_score("src/main.rs", "xyz").is_none());
        // 边界/连续命中应比分散命中得分高
        let boundary = fuzzy_score("src/main.rs", "main").unwrap();   // 在 `/` 边界后连续
        let scattered = fuzzy_score("seamain.rs", "main").unwrap();
        assert!(boundary > scattered, "边界连续应更高: {boundary} vs {scattered}");
        // smart-case：含大写则区分
        assert!(fuzzy_score("readme.md", "R").is_none());
        assert!(fuzzy_score("README.md", "R").is_some());
    }

    #[test]
    fn expand_mentions_injects_real_file_content() {
        let dir = std::env::temp_dir().join(format!("nekofc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "HELLO_CONTENT").unwrap();

        // 真实文件 → 注入文件内容（对照 CC）
        let out = expand_mentions("看 @hello.txt 这个", &dir);
        assert!(out.contains("HELLO_CONTENT"), "应注入文件内容: {out}");
        assert!(out.contains("hello.txt"));

        // 不存在的引用 → 忽略
        assert!(expand_mentions("@nope.txt", &dir).is_empty());

        // 无 @ → 空
        assert!(expand_mentions("just text", &dir).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_url_to_path_parses_standard_urls() {
        // Linux 本地
        assert_eq!(
            file_url_to_path("file:///home/user/image.png").as_deref(),
            Some("/home/user/image.png"),
        );
        // localhost
        assert_eq!(
            file_url_to_path("file://localhost/home/user/image.png").as_deref(),
            Some("/home/user/image.png"),
        );
        // Windows 盘符
        assert_eq!(
            file_url_to_path("file:///C:/Users/image.png").as_deref(),
            Some("C:/Users/image.png"),
        );
    }

    #[test]
    fn file_url_to_path_parses_unc_paths() {
        // WSL 旧格式
        assert_eq!(
            file_url_to_path("file://wsl$/Ubuntu/home/user/file.txt").as_deref(),
            Some("//wsl$/Ubuntu/home/user/file.txt"),
        );
        // WSL 新格式
        assert_eq!(
            file_url_to_path("file://wsl.localhost/Ubuntu/home/user/file.txt").as_deref(),
            Some("//wsl.localhost/Ubuntu/home/user/file.txt"),
        );
        // SMB 网络路径
        assert_eq!(
            file_url_to_path("file://server/share/file.txt").as_deref(),
            Some("//server/share/file.txt"),
        );
    }

    #[test]
    fn file_url_to_path_decodes_percent_encoding() {
        assert_eq!(
            file_url_to_path("file:///home/user/my%20file.png").as_deref(),
            Some("/home/user/my file.png"),
        );
        assert_eq!(
            file_url_to_path("file:///home/user/%E4%B8%AD%E6%96%87.txt").as_deref(),
            Some("/home/user/中文.txt"),
        );
    }

    #[test]
    fn file_url_to_path_rejects_non_file_urls() {
        assert!(file_url_to_path("https://example.com/file.png").is_none());
        assert!(file_url_to_path("/home/user/file.png").is_none());
        assert!(file_url_to_path("plain text").is_none());
    }

    #[test]
    fn process_paste_converts_single_file_url() {
        let out = process_paste("file:///home/user/image.png");
        assert_eq!(out, "@/home/user/image.png ");
    }

    #[test]
    fn process_paste_converts_multiple_file_urls() {
        let out = process_paste("file:///home/user/a.png\nfile:///home/user/b.png");
        assert_eq!(out, "@/home/user/a.png @/home/user/b.png ");
    }

    #[test]
    fn process_paste_preserves_plain_text() {
        assert_eq!(process_paste("hello world"), "hello world");
        assert_eq!(process_paste("check this file:///path/to/file.txt out"), "check this file:///path/to/file.txt out");
    }

    #[test]
    fn process_paste_handles_empty() {
        assert_eq!(process_paste(""), "");
    }
}
