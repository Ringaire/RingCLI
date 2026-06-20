//! Provider 连接 / 模型切换的共享核心逻辑（TUI 与 plain REPL 复用）。
//!
//! 只改 `BootstrappedRuntime`（provider / model / registry / config），**不碰 ctx 与 UI**——
//! 由各前端读返回值后自行更新会话上下文并渲染消息。

use std::path::Path;
use std::sync::{Arc, OnceLock};

use neko_core::{load_config, load_user_config, save_config, NekoUserConfig, ProviderEntry};

use crate::bootstrap::BootstrappedRuntime;

/// 串行化配置文件的读-改-写，避免后台模型缓存（`cache_models`）与 `/connect` 落盘并发互相覆盖。
fn config_write_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// `/model` 切换结果（前端据此渲染）。
pub enum SwitchResult {
    /// 切换了 provider + model。
    Switched { provider: String, model: String },
    /// 仅改 model 名（输入无 provider 前缀）。
    ModelOnly { model: String },
    /// 指定的 provider 未注册。
    ProviderMissing { provider: String },
    /// 当前未配置任何 provider，裸 model 名无意义。
    NoProvider,
}

/// `/connect` 快速连接结果。
pub enum ConnectResult {
    Connected { provider: String, model: String },
    /// 失败（已成文案，前端直接展示）。
    Rejected(String),
}

/// 执行 `/model` 切换：更新 `runtime` 的 provider/model 并重建上下文（catalog + 系统提示词），
/// 返回结果供前端同步 ctx 并渲染。
pub async fn switch_model(runtime: &mut BootstrappedRuntime, model_ref: &str) -> SwitchResult {
    let (prov_hint, model_name) = neko_providers::split_model_ref(model_ref);
    match prov_hint {
        Some(prov_id) => match runtime.provider_registry.get(&prov_id) {
            Some(p) => {
                runtime.provider = Some(p);
                runtime.model = model_name.clone();
                runtime.rebuild_context().await;
                SwitchResult::Switched { provider: prov_id, model: model_name }
            }
            None => SwitchResult::ProviderMissing { provider: prov_id },
        },
        None => {
            // 无 provider 时裸 model 名无处可用——不静默设值误导用户。
            if runtime.provider.is_none() {
                return SwitchResult::NoProvider;
            }
            runtime.model = model_name.clone();
            runtime.rebuild_context().await;
            SwitchResult::ModelOnly { model: model_name }
        }
    }
}

/// `/connect <provider> <key> [url]` 快速配置：校验 + 写配置 + 热重载。
pub async fn quick_connect(
    runtime:  &mut BootstrappedRuntime,
    cwd:      &Path,
    provider: &str,
    api_key:  Option<String>,
    base_url: Option<String>,
) -> ConnectResult {
    let catalog = neko_providers::catalog::load(
        Some(&neko_core::session::paths::config_dir()),
        Some(cwd),
    );
    let Some(entry) = neko_providers::catalog::get(&catalog, provider).cloned() else {
        return ConnectResult::Rejected(format!(
            "Unknown provider: \"{provider}\". Run /connect to open the setup wizard."
        ));
    };
    let key_missing = api_key.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true);
    if entry.api_key_env.is_some() && key_missing {
        return ConnectResult::Rejected(format!(
            "Usage: /connect {provider} <apiKey> [baseUrl]   (or /connect for the wizard)"
        ));
    }
    let Some(model) = entry.default_model.clone() else {
        return ConnectResult::Rejected(format!("No default model for '{provider}'; use /connect to pick one."));
    };

    let mut cfg = load_user_config().await;
    let providers = cfg.providers.get_or_insert_with(Default::default);
    let mut pe = ProviderEntry::default();
    if let Some(k) = api_key.filter(|s| !s.trim().is_empty()) { pe.api_key = Some(k); }
    if let Some(u) = base_url.filter(|s| !s.trim().is_empty()) { pe.base_url = Some(u); }
    providers.insert(provider.to_string(), pe);
    cfg.model = Some(format!("{provider}/{model}"));

    match apply_config_reload(runtime, cwd, &cfg, provider, &model).await {
        Ok(())  => ConnectResult::Connected { provider: provider.to_string(), model },
        Err(e)  => ConnectResult::Rejected(e),
    }
}

/// 把拉取到的模型 id 列表缓存进全局配置的 `models[provider]`（对照 bun 的 `/model refresh`），
/// 供 `/model` 下次即时展示，无需再等网络。空列表不写。
pub async fn cache_models(provider_id: &str, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    // 持锁完成 load→改→save，确保读到的是最新配置（不会用旧快照覆盖并发的 /connect 写入）。
    let _guard = config_write_lock().lock().await;
    let mut cfg = load_user_config().await;
    cfg.models
        .get_or_insert_with(Default::default)
        .insert(provider_id.to_string(), ids.to_vec());
    let _ = save_config(&cfg).await;
}

/// 落盘 `cfg` 并重建 provider 注册表 + 热替换当前 provider/model/config（不动 ctx/UI）。
///
/// 向导（`finish_setup`）与 `quick_connect` 共用。成功后 `runtime.provider` 必为 `Some`。
pub async fn apply_config_reload(
    runtime:     &mut BootstrappedRuntime,
    cwd:         &Path,
    cfg:         &NekoUserConfig,
    provider_id: &str,
    model:       &str,
) -> Result<(), String> {
    {
        // 与 cache_models 共用写锁，避免后台模型缓存覆盖刚连上的 provider/model。
        let _guard = config_write_lock().lock().await;
        save_config(cfg).await.map_err(|e| format!("save failed: {e}"))?;
    }

    let resolved = load_config(Some(cwd)).await;
    let boot = neko_providers::build_registry(&resolved);
    let registry = Arc::new(boot.registry);
    let provider = registry.get(provider_id)
        .ok_or_else(|| format!("provider '{provider_id}' not usable after save"))?;

    runtime.provider          = Some(provider);
    runtime.provider_registry = registry;
    runtime.model             = model.to_string();
    runtime.config            = resolved;
    // 重建子 agent 目录与系统提示词（冷启动后二者基于空 provider，必须刷新）。
    runtime.rebuild_context().await;
    Ok(())
}
