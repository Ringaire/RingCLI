// 启动引导：装配 config、provider、tools、MCP、skills、permissions、session

use std::sync::Arc;
use anyhow::{anyhow, Context, Result};
use tokio::sync::Mutex;
use tracing::{debug, info};

use neko_core::agent::ModelCatalogEntry;
use neko_core::config::ResolvedConfig;
use neko_core::events::EventBus;
use neko_core::permissions::{DefaultPermissionEngine, ModeName};
use neko_core::session::{self, Session};
use neko_core::skills::SkillRegistry;
use neko_core::tools::ToolRegistry;
use neko_core::NekoRuntime;

use neko_providers::provider::Provider;
use neko_providers::ProviderRegistry;

use neko_engine::build_system_prompt;
use crate::args::Args;
use crate::mcp_manager::CliMcpManager;

/// 装配完成的运行时，包含 REPL/TUI 运行所需的一切。
pub struct BootstrappedRuntime {
    pub bus:               EventBus,
    pub tools:             Arc<dyn ToolRegistry>,
    pub skills:            Arc<SkillRegistry>,
    pub permissions:       Arc<Mutex<DefaultPermissionEngine>>,
    /// 当前 provider。`None` = 未配置任何可用 provider（冷启动），UI 须进入 setup-required 态。
    pub provider:          Option<Arc<dyn Provider>>,
    pub provider_registry: Arc<ProviderRegistry>,
    pub model:             String,
    pub mode:              ModeName,
    /// 子 agent 模型目录（按 role 分类），供 orchestrator 选模
    pub catalog:           Vec<ModelCatalogEntry>,
    /// 解析后的配置；供后续命令（如 /model 列表、provider 切换）查询
    pub config:            ResolvedConfig,
    pub session:           Session,
    pub cwd:               std::path::PathBuf,
    pub system_prompt:     String,
    pub skip_perms:        bool,
    /// 会话级运行时：工具注册表 + MCP 动态管理 + 配置热重载。
    /// 持有以保活 MCP server 与运行时状态；亦供后续 /mcp 管理命令使用。
    #[allow(dead_code)]
    pub neko_runtime:      Arc<NekoRuntime>,
    /// 保活：配置文件监听器（drop 即停止热重载）。
    #[allow(dead_code)]
    pub config_watcher:    Option<notify::RecommendedWatcher>,
}

impl BootstrappedRuntime {
    /// provider / model 变更后（如 `/connect` 热重载）重建子 agent 模型目录与系统提示词。
    ///
    /// 冷启动时 provider 为 None → catalog 空、system_prompt 用占位 model；连接成功后须重建，
    /// 否则 orchestrator 选模目录为空、提示词里 current_model 为空，直到重启才恢复。
    pub async fn rebuild_context(&mut self) {
        let model = self.model.clone();
        self.catalog = build_catalog(&self.config, self.provider.as_deref(), &model).await;
        let base = neko_engine::build_system_prompt(
            self.cwd.as_path(),
            self.tools.as_ref(),
            &self.skills,
            &model,
            &self.mode.to_string(),
        ).await;
        self.system_prompt =
            neko_engine::agent::orchestrator::build_orchestrator_prompt(&self.catalog, &model, &base);
    }
}

/// 执行完整的启动引导。
/// `session_id`：从 CLI 解析好的 UUID（如 --resume），None 则新建会话。
pub async fn bootstrap(args: &Args, session_id: Option<uuid::Uuid>) -> Result<BootstrappedRuntime> {
    let cwd = match &args.cwd {
        Some(p) => p.clone(),
        None    => std::env::current_dir().context("failed to get current directory")?,
    };

    // ── 1. 加载配置 ──
    let config = neko_core::load_config(Some(&cwd)).await;
    debug!(providers = ?neko_providers::factory::summarize(&config), "config loaded");

    // ── 2. 构建 provider 注册表 ──
    let bootstrap_p = neko_providers::build_registry(&config);
    let provider_registry = Arc::new(bootstrap_p.registry);

    // ── 3. 解析 provider + model ──
    let (provider, model) = resolve_provider_and_model(
        args,
        &config,
        &provider_registry,
        bootstrap_p.default_provider_id.as_deref(),
    )?;

    // ── 4. 权限模式 ──
    let mode: ModeName = args.mode.parse()
        .map_err(|e: String| anyhow!("invalid --mode: {e}"))?;
    let mut perm_engine = DefaultPermissionEngine::new(mode);
    if args.dangerously_skip_permissions {
        perm_engine.dangerously_skip_permissions();
    }
    let permissions = Arc::new(Mutex::new(perm_engine));

    // ── 5. 运行时（工具注册表 + 事件总线 + MCP 管理）──
    let neko_runtime = Arc::new(NekoRuntime::new());
    neko_runtime.set_mcp_manager(Arc::new(CliMcpManager::new()));
    let bus = neko_runtime.bus.clone();

    // 内置工具
    neko_tools::register_all(neko_runtime.tools.as_ref());

    // ── 6. MCP 服务器（通过运行时动态加载，支持后续热重载）──
    neko_runtime.apply_mcp_config(&config.mcp_servers).await;

    let tools: Arc<dyn ToolRegistry> = neko_runtime.tools_dyn();

    // ── 7. 技能（内置 → 全局目录 → 项目级 .agents/skills + .neko/skills）──
    let mut skill_registry = SkillRegistry::new();
    neko_skills::load_builtin_skills(&mut skill_registry);

    // 全局目录：~/.local/share/neko/skills/（旧 JSON + 新 SKILL.md 兼容）
    let global_skills_dir = neko_core::session::paths::skills_dir();
    neko_skills::load_skills_from_dir(&mut skill_registry, &global_skills_dir).await;

    // 全局目录：~/.config/neko/skills/
    let config_skills_dir = neko_core::session::paths::config_dir().join("skills");
    neko_skills::load_skills_from_dir(&mut skill_registry, &config_skills_dir).await;

    // 项目级：.agents/skills/（CC/opencode 兼容）
    let agents_skills_dir = cwd.join(".agents").join("skills");
    neko_skills::load_skills_from_dir(&mut skill_registry, &agents_skills_dir).await;

    // 项目级：.neko/skills/
    let neko_skills_dir = cwd.join(".neko").join("skills");
    neko_skills::load_skills_from_dir(&mut skill_registry, &neko_skills_dir).await;

    let skills = Arc::new(skill_registry);

    // ── 8. 会话（新建或恢复）──
    let session = match session_id {
        Some(id) => {
            session::load_session(id).await
                .ok_or_else(|| anyhow!("session not found: {id}"))?
        }
        None => {
            let m = if model.is_empty() { None } else { Some(model.clone()) };
            session::create_session(cwd.clone(), m).await
        }
    };

    // ── 9. 子 agent 模型目录（orchestrator 选模用）──
    let catalog = build_catalog(&config, provider.as_deref(), &model).await;

    // ── 10. 系统提示词（基础 + 编排段）──
    let base_prompt = build_system_prompt(&cwd, tools.as_ref(), &skills, &model, &args.mode).await;
    let system_prompt = neko_engine::agent::orchestrator::build_orchestrator_prompt(&catalog, &model, &base_prompt);

    // ── 11. 配置热重载监听 ──
    let config_watcher = crate::config_watch::spawn_config_watch(
        neko_runtime.clone(),
        bus.clone(),
        cwd.clone(),
        session.meta.id,
    );

    info!(
        provider = provider.as_ref().map(|p| p.id().to_string()).unwrap_or_else(|| "(none)".to_string()),
        model = %model,
        mode = %mode,
        tools = tools.list().len(),
        skills = skills.list().len(),
        mcp = neko_runtime.mcp_server_names().len(),
        catalog = catalog.len(),
        hot_reload = config_watcher.is_some(),
        "runtime bootstrapped"
    );

    Ok(BootstrappedRuntime {
        bus,
        tools,
        skills,
        permissions,
        provider,
        provider_registry,
        model,
        mode,
        catalog,
        config,
        session,
        cwd,
        system_prompt,
        skip_perms: args.dangerously_skip_permissions,
        neko_runtime,
        config_watcher,
    })
}

/// 构建子 agent 模型目录：优先用 config.models[provider]，否则用 provider 的内置模型列表，
/// 再否则退化为仅当前模型。
async fn build_catalog(
    config:   &ResolvedConfig,
    provider: Option<&dyn Provider>,
    model:    &str,
) -> Vec<ModelCatalogEntry> {
    // 未配置 provider：无目录（setup 完成后由热重载路径重建）。
    let Some(provider) = provider else {
        return Vec::new();
    };
    // 1. config 声明的该 provider 模型列表
    if let Some(ids) = config.models.get(provider.id()) {
        if !ids.is_empty() {
            return neko_core::build_model_catalog(ids);
        }
    }
    // 2. provider 内置已知模型
    if let Ok(models) = provider.list_models().await {
        if !models.is_empty() {
            let ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
            return neko_core::build_model_catalog(&ids);
        }
    }
    // 3. 仅当前模型
    neko_core::build_model_catalog(&[model.to_string()])
}

/// 解析最终使用的 provider 与 model 名。优先级：args > config > 默认。
///
/// **不预设任何 provider**：当没有任何可用 provider（冷启动、未配置 key）时返回 `(None, "")`，
/// 由 UI 进入 setup-required 态、提示用户 `/connect`，而非报错退出或回退到某家提供商。
fn resolve_provider_and_model(
    args:        &Args,
    config:      &ResolvedConfig,
    registry:    &ProviderRegistry,
    default_id:  Option<&str>,
) -> Result<(Option<Arc<dyn Provider>>, String)> {
    // model 字符串可能形如 "provider/model"
    let config_model = config.model.clone();
    let (model_prov_hint, model_name_hint) = match (&args.model, &config_model) {
        (Some(m), _) => neko_providers::split_model_ref(m),
        (None, Some(m)) => neko_providers::split_model_ref(m),
        (None, None) => (None, String::new()),
    };

    // provider id：args.provider > model 前缀 > 注册表推断的默认。三者皆无 → 未配置。
    let Some(provider_id) = args.provider.clone()
        .or(model_prov_hint)
        .or_else(|| default_id.map(|s| s.to_string()))
    else {
        return Ok((None, String::new()));
    };

    // 指定了 provider 但注册表里没有（无 key / 未配置）→ 同样视为未配置。
    let Some(provider) = registry.get(&provider_id) else {
        return Ok((None, String::new()));
    };

    // model 名：hint 非空则用，否则 provider 默认
    let model = if !model_name_hint.is_empty() {
        model_name_hint
    } else {
        provider.default_model().to_string()
    };

    Ok((Some(provider), model))
}
