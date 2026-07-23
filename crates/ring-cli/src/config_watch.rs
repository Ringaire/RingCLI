// 配置文件热重载：监听配置文件变化，重新加载并对 MCP server 做增量 apply。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher};
use tracing::{debug, info, warn};

use ring_core::events::{EventBus, RingEvent};
use ring_core::RingRuntime;
use uuid::Uuid;

const DEBOUNCE_MS: u64 = 300;

/// 启动配置热重载监听任务。返回 watcher 句柄（需保活，drop 即停止监听）。
///
/// cwd 用于解析项目级配置；session_id 用于事件标注。
pub fn spawn_config_watch(
    runtime:    Arc<RingRuntime>,
    bus:        EventBus,
    cwd:        PathBuf,
    session_id: Uuid,
) -> Option<notify::RecommendedWatcher> {
    let config_path = ring_core::session::paths::config_path();

    // notify 回调是同步的：通过 tokio mpsc 把事件转给异步任务处理
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
        match res {
            Ok(ev) if is_modify(&ev) => {
                let _ = tx.send(());
            }
            Ok(_) => {}
            Err(e) => warn!(err = %e, "config watcher error"),
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            warn!(err = %e, "failed to create config watcher; hot-reload disabled");
            return None;
        }
    };

    // 监听全局配置文件所在目录（文件可能被原子替换 → 监目录更可靠）
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
        if let Err(e) = watcher.watch(parent, RecursiveMode::NonRecursive) {
            warn!(err = %e, "failed to watch config dir; hot-reload disabled");
            return None;
        }
    }
    // 项目级 .ring 目录（若存在）
    let project_dir = cwd.join(".ring");
    if project_dir.is_dir() {
        let _ = watcher.watch(&project_dir, RecursiveMode::NonRecursive);
    }

    // 异步重载任务
    let reload_cwd = cwd.clone();
    tokio::spawn(async move {
        loop {
            // 等待一个变更信号
            if rx.recv().await.is_none() {
                break;
            }
            // 防抖：吸收紧随其后的连续事件
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)) => break,
                    msg = rx.recv() => {
                        if msg.is_none() { return; }
                        // 收到更多事件，继续吸收
                    }
                }
            }

            debug!("config change detected; reloading");
            let cfg = ring_core::load_config(Some(&reload_cwd)).await;
            runtime.apply_mcp_config(&cfg.mcp_servers).await;
            info!(mcp = runtime.mcp_server_names().len(), "config hot-reloaded");
            bus.emit(RingEvent::SessionMessage {
                session_id,
                role:    "system".to_string(),
                content: format!(
                    "config reloaded ({} MCP server(s) active)",
                    runtime.mcp_server_names().len()
                ),
            });
        }
    });

    Some(watcher)
}

fn is_modify(ev: &Event) -> bool {
    use notify::EventKind;
    matches!(
        ev.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
    )
}
