use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tracing::{debug, warn};
use uuid::Uuid;

use crate::session::paths;

const COMPRESSED_SUFFIX: &str = ".zst";
const COMPRESSION_LEVEL: i32 = 3;
const MIN_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

// ── 路径助手 ─────────────────────────────────────────────────────────────────

pub fn jsonl_path(session_id: Uuid) -> PathBuf {
    paths::sessions_dir().join(format!("{session_id}.jsonl"))
}

pub fn zst_path(session_id: Uuid) -> PathBuf {
    paths::sessions_dir().join(format!("{session_id}.jsonl{COMPRESSED_SUFFIX}"))
}

/// 返回实际存在的 JSONL 文件路径（自动检测 .zst 压缩）。
pub fn existing_jsonl(session_id: Uuid) -> Option<PathBuf> {
    let plain = jsonl_path(session_id);
    if plain.exists() {
        return Some(plain);
    }
    let zst = zst_path(session_id);
    if zst.exists() {
        return Some(zst);
    }
    None
}

/// 透明读取 JSONL 内容 — 自动处理 .zst 压缩文件。
pub fn read_jsonl(session_id: Uuid) -> std::io::Result<String> {
    let plain = jsonl_path(session_id);
    if plain.exists() {
        return std::fs::read_to_string(&plain);
    }

    let zst = zst_path(session_id);
    if zst.exists() {
        return decompress_to_string(&zst);
    }

    Ok(String::new())
}

// ── 压缩/解压 ────────────────────────────────────────────────────────────────

/// 将 .jsonl 压缩为 .jsonl.zst，成功后删除原文件。
pub fn compress_file(input: &Path) -> std::io::Result<PathBuf> {
    let output = input.with_extension(format!(
        "{}{}",
        input.extension().and_then(|e| e.to_str()).unwrap_or(""),
        COMPRESSED_SUFFIX
    ));

    debug!("compressing {} -> {}", input.display(), output.display());

    let raw = std::fs::read(input)?;
    let compressed = zstd::encode_all(raw.as_slice(), COMPRESSION_LEVEL)?;

    // 原子写入：先写临时文件，再 rename
    let tmp = output.with_extension("tmp");
    std::fs::write(&tmp, &compressed)?;
    std::fs::rename(&tmp, &output)?;

    // 验证压缩完整性
    match zstd::decode_all(compressed.as_slice()) {
        Ok(decoded) if decoded == raw => {
            std::fs::remove_file(input)?;
            debug!("compression verified, original removed");
        }
        Ok(_) => {
            warn!("compression verification failed (data mismatch), keeping original");
            let _ = std::fs::remove_file(&output);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "zstd round-trip mismatch",
            ));
        }
        Err(e) => {
            warn!("compression verification failed: {e}");
            let _ = std::fs::remove_file(&output);
            return Err(e);
        }
    }

    Ok(output)
}

/// 将 .jsonl.zst 解压为 .jsonl，成功后删除 .zst 文件。
pub fn decompress_file(input: &Path) -> std::io::Result<PathBuf> {
    let output = input.with_extension("");

    debug!("decompressing {} -> {}", input.display(), output.display());

    let compressed = std::fs::read(input)?;
    let raw = zstd::decode_all(compressed.as_slice())?;

    let tmp = output.with_extension("tmp");
    std::fs::write(&tmp, &raw)?;
    std::fs::rename(&tmp, &output)?;
    std::fs::remove_file(input)?;

    Ok(output)
}

fn decompress_to_string(path: &Path) -> std::io::Result<String> {
    let compressed = std::fs::read(path)?;
    let raw = zstd::decode_all(compressed.as_slice())?;
    String::from_utf8(raw).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

// ── 条件检查 ─────────────────────────────────────────────────────────────────

/// 判断文件是否足够旧（超过 MIN_AGE）值得压缩。
fn is_old_enough(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    age >= MIN_AGE
}

// ── 后台 worker ──────────────────────────────────────────────────────────────

/// 扫描 sessions 目录，压缩符合条件的旧 .jsonl 文件。
/// 此函数是同步阻塞的，应由调用方在 `spawn_blocking` 中运行。
pub fn run_compression_pass() {
    let dir = paths::sessions_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    let mut compressed_count = 0u32;

    for entry in entries.flatten() {
        let path = entry.path();

        // 只处理 .jsonl 文件（跳过 .jsonl.zst、.meta.json 等）
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        // 排除 .meta.json
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".meta.json") {
            continue;
        }

        if !is_old_enough(&path) {
            continue;
        }

        match compress_file(&path) {
            Ok(compressed_path) => {
                // 从文件名解析 UUID
                if let Some(id_str) = name.strip_suffix(".jsonl") {
                    if let Ok(uuid) = Uuid::parse_str(id_str) {
                        crate::session::db::set_compressed(uuid, true);
                    }
                }
                debug!("compressed {}", compressed_path.display());
                compressed_count += 1;
            }
            Err(e) => {
                warn!("failed to compress {}: {e}", path.display());
            }
        }
    }

    if compressed_count > 0 {
        debug!("compression pass: {compressed_count} files compressed");
    }
}

/// 解压指定 session 的 JSONL（如果已压缩），用于 append 前恢复可写状态。
pub fn ensure_decompressed(session_id: Uuid) -> std::io::Result<PathBuf> {
    let zst = zst_path(session_id);
    if zst.exists() {
        let plain = decompress_file(&zst)?;
        crate::session::db::set_compressed(session_id, false);
        return Ok(plain);
    }
    Ok(jsonl_path(session_id))
}
