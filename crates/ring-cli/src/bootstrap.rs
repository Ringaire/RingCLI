// 启动引导：装配 config、provider、tools、MCP、skills、permissions、session

use std::sync::Arc;
use anyhow::{anyhow, Context, Result};
use parking_lot::RwLock;
use tokio::sync::Mutex;
use tracing::{debug, info};

use ring_core::agent::ModelCatalogEntry;
use ring_core::config::ResolvedConfig;
use ring_core::events::EventBus;
use ring_core::permissions::{DefaultPermissionEngine, ModeName};
use ring_core::session::{self, Session};
use ring_core::skills::SkillRegistry;
use ring_core::tools::ToolRegistry;
use ring_core::NekoRuntime;

use ring_providers::provider::Provider;
use ring_providers::ProviderRegistry;

use ring_engine::build_system_prompt;
use crate::args::Args;
use crate::mcp_manager::CliMcpManager;

/// 装配完成的运行时，包含 REPL/TUI 运行所需的一切。
pub struct BootstrappedRuntime {
    pub bus:               EventBus,
    pub tools:             Arc<dyn ToolRegistry>,
    pub skills:            Arc<RwLock<SkillRegistry>>,
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
    pub ring_runtime:      Arc<NekoRuntime>,
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
        let skills_xml = self.skills.read().build_available_skills();
        let base = ring_engine::build_system_prompt(
            self.cwd.as_path(),
            self.tools.as_ref(),
            &skills_xml,
            &model,
            &self.mode.to_string(),
        ).await;
        self.system_prompt =
            ring_engine::agent::orchestrator::build_orchestrator_prompt(&self.catalog, &model, &base);
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
    let config = ring_core::load_config(Some(&cwd)).await;
    debug!(providers = ?ring_providers::factory::summarize(&config), "config loaded");

    // ── 2. 构建 provider 注册表 ──
    let bootstrap_p = ring_providers::build_registry(&config);
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

    // ── 5. 技能（内置 → 全局目录 → 项目级）── 提前加载，MCP prompts import 需要
    let mut skill_registry = SkillRegistry::new();
    ring_skills::load_builtin_skills(&mut skill_registry);

    // 全局目录：~/.local/share/ring/skills/（旧 JSON + 新 SKILL.md 兼容）
    let global_skills_dir = ring_core::session::paths::skills_dir();
    ring_skills::load_skills_from_dir(&mut skill_registry, &global_skills_dir).await;

    // 全局目录：~/.config/ring/skills/
    let config_skills_dir = ring_core::session::paths::config_dir().join("skills");
    ring_skills::load_skills_from_dir(&mut skill_registry, &config_skills_dir).await;

    // 项目级：.agents/skills/（CC/opencode 兼容）
    let agents_skills_dir = cwd.join(".agents").join("skills");
    ring_skills::load_skills_from_dir(&mut skill_registry, &agents_skills_dir).await;

    // 项目级：.ring/skills/
    let ring_skills_dir = cwd.join(".ring").join("skills");
    ring_skills::load_skills_from_dir(&mut skill_registry, &ring_skills_dir).await;

    // 项目级：.ring/doc/（新标准路径，兼容 skill 命名）
    let ring_doc_dir = cwd.join(".ring").join("doc");
    ring_skills::load_skills_from_dir(&mut skill_registry, &ring_doc_dir).await;

    // 项目级：.ring/doc/（兼容）
    let ring_doc_dir = cwd.join(".ring").join("doc");
    ring_skills::load_skills_from_dir(&mut skill_registry, &ring_doc_dir).await;

    // 全局目录：~/.ring/doc/（新标准路径）
    let global_doc_dir = ring_core::session::paths::doc_dir();
    ring_skills::load_skills_from_dir(&mut skill_registry, &global_doc_dir).await;

    let skills: Arc<RwLock<SkillRegistry>> = Arc::new(RwLock::new(skill_registry));

    // ── 6. 运行时（工具注册表 + 事件总线 + MCP 管理，共享 skills）──
    let mut ring_rt = NekoRuntime::new_with_tools(ring_tools::init_hybrid_registry());
    ring_rt.skills = skills.clone();
    let ring_runtime = Arc::new(ring_rt);
    ring_runtime.set_mcp_manager(Arc::new(CliMcpManager::new(skills.clone())));
    let bus = ring_runtime.bus.clone();

    // ── 7. MCP 服务器（加载时自动把 server 的 prompts import 到 skills）──
    ring_runtime.apply_mcp_config(&config.mcp_servers).await;

    let tools: Arc<dyn ToolRegistry> = ring_runtime.tools_dyn();

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
    let skills_xml = skills.read().build_available_skills();
    let base_prompt = build_system_prompt(&cwd, tools.as_ref(), &skills_xml, &model, &args.mode).await;
    let system_prompt = ring_engine::agent::orchestrator::build_orchestrator_prompt(&catalog, &model, &base_prompt);

    // ── 11. 配置热重载监听 ──
    let config_watcher = crate::config_watch::spawn_config_watch(
        ring_runtime.clone(),
        bus.clone(),
        cwd.clone(),
        session.meta.id,
    );

    info!(
        provider = provider.as_ref().map(|p| p.id().to_string()).unwrap_or_else(|| "(none)".to_string()),
        model = %model,
        mode = %mode,
        tools = tools.list().len(),
        skills = skills.read().list().len(),
        mcp = ring_runtime.mcp_server_names().len(),
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
        ring_runtime,
        config_watcher,
    })
}

/// 构建子 agent 模型目录：优先用 config.models[provider]，否则用 provider 的内置模型列表，
/// 再否则退化为仅当前模型。
pub async fn build_catalog(
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
            return ring_core::build_model_catalog(ids);
        }
    }
    // 2. provider 内置已知模型
    if let Ok(models) = provider.list_models().await {
        if !models.is_empty() {
            let ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
            return ring_core::build_model_catalog(&ids);
        }
    }
    // 3. 仅当前模型
    ring_core::build_model_catalog(&[model.to_string()])
}

/// 解析最终使用的 provider 与 model 名。优先级：args > config > 默认。
///
/// **不预设任何 provider**：当没有任何可用 provider（冷启动、未配置 key）时返回 `(None, "")`，
/// 由 UI 进入 setup-required 态、提示用户 `/connect`，而非报错退出或回退到某家提供商。
pub fn resolve_provider_and_model(
    args:        &Args,
    config:      &ResolvedConfig,
    registry:    &ProviderRegistry,
    default_id:  Option<&str>,
) -> Result<(Option<Arc<dyn Provider>>, String)> {
    // model 字符串可能形如 "provider/model"
    let config_model = config.model.clone();
    let (model_prov_hint, model_name_hint) = match (&args.model, &config_model) {
        (Some(m), _) => ring_providers::split_model_ref(m),
        (None, Some(m)) => ring_providers::split_model_ref(m),
        (None, None) => (None, String::new()),
    };

    // provider id：args.provider > model 前缀 > 注册表推断的默认。三者皆无 → 未配置。
    let Some(provider_id) = args.provider.clone()
        .or(model_prov_hint)
        .or_else(|| default_id.map(|s| s.to_string()))
    else {
        debug!("no provider id resolved");
        return Ok((None, String::new()));
    };
    
    debug!(provider_id = %provider_id, "resolved provider id");

    // 指定了 provider 但注册表里没有（无 key / 未配置）→ 同样视为未配置。
    let Some(provider) = registry.get(&provider_id) else {
        debug!(provider_id = %provider_id, available = ?registry.list().iter().map(|p| p.id()).collect::<Vec<_>>(), "provider not found in registry");
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
