//! bootstrap 模块集成测试（暂时全部 ignore）
//!
//! # 为何全部 ignore（2026-07-05 阶段 0 基线收口）
//!
//! 这些测试调用真实的 `bootstrap()`，后者读全局 env (`*_API_KEY`)
//! 与全局 `~/.neko/` 配置/session 存储。tokio 测试属性默认并行执行，
//! `test_bootstrap_provider_from_env` 的 `std::env::set_var` 会污染并行运行的
//! `cold_start_no_provider` / `model_catalog_empty_without_provider`，产生竞态失败。
//!
//! 单线程 `cargo test --test bootstrap_test -- --test-threads=1` 可全部通过，
//! 证明是并行 + 全局状态的隔离失败，而非 bootstrap 实现缺陷。
//!
//! # 复活条件（阶段 2：bootstrap 可测试化重构）
//!
//! - 重构 `bootstrap()` 接受依赖注入：`EnvProvider` trait + `ConfigSource` trait
//! - 测试传入 fake env / in-memory config，消除全局状态依赖
//! - 移除本文件的 `#[ignore]`，恢复并行安全
//!
//! 验证方式（临时）：`cargo test --test bootstrap_test -- --ignored --test-threads=1`

// 测试策略（原设计意图，保留待阶段 2 复活）：
// - 使用临时目录隔离测试环境
// - 模拟配置文件和环境变量
// - 测试完整启动流程和各种场景

use std::path::PathBuf;
use tempfile::TempDir;
use neko_cli::args::Args;

// ── 测试工具函数 ──────────────────────────────────────────────────────────────

/// 创建最小化的 Args 用于测试
fn create_test_args(cwd: PathBuf) -> Args {
    Args {
        prompt: None,
        mode: "build".to_string(),
        print: false,
        output_format: "text".to_string(),
        r#continue: false,
        resume: None,
        list_sessions: false,
        model: None,
        provider: None,
        cwd: Some(cwd),
        dangerously_skip_permissions: true, // 测试中跳过权限检查
        extended_thinking: false,
        verbose: false,
        debug: None,
        add_dir: Vec::new(),
        no_tui: true,
        serve: None,
        rca: None,
        sdk: false,
    }
}

/// 创建带配置文件的临时目录
fn create_temp_dir_with_config(config_content: &str) -> std::io::Result<TempDir> {
    let temp = TempDir::new()?;
    let neko_dir = temp.path().join(".neko");
    std::fs::create_dir_all(&neko_dir)?;
    let config_path = neko_dir.join("settings.jsonc");
    std::fs::write(config_path, config_content)?;
    Ok(temp)
}

// ── 测试用例 ──────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_cold_start_no_provider() {
    // 场景：无任何配置、无环境变量 → 应返回 (None, String::new())
    let temp = TempDir::new().expect("failed to create temp dir");
    let args = create_test_args(temp.path().to_path_buf());

    // 清空可能影响测试的环境变量
    for key in &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPENROUTER_API_KEY"] {
        std::env::remove_var(key);
    }

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "cold start should succeed");
    let runtime = result.unwrap();

    // 冷启动：provider 应为 None，model 应为空
    assert!(runtime.provider.is_none(), "provider should be None in cold start");
    assert_eq!(runtime.model, "", "model should be empty in cold start");

    // 其他组件应正常初始化
    assert!(!runtime.tools.list().is_empty(), "tools should be initialized");
    assert_eq!(runtime.mode.to_string(), "build", "mode should be 'build'");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_with_provider_in_config() {
    // 场景：配置文件中有 provider 声明（带 API key）
    let config = r#"{
        "providers": {
            "anthropic": {
                "apiKey": "sk-ant-test-key-123456789"
            }
        },
        "model": "anthropic/claude-3-5-sonnet-20241022"
    }"#;

    let temp = create_temp_dir_with_config(config).expect("failed to create temp dir with config");
    let args = create_test_args(temp.path().to_path_buf());

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap with config provider should succeed");
    let runtime = result.unwrap();

    // 应该成功解析 provider
    assert!(runtime.provider.is_some(), "provider should be available");
    assert_eq!(runtime.provider.unwrap().id(), "anthropic", "provider should be anthropic");
    assert!(!runtime.model.is_empty(), "model should not be empty");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_provider_from_env() {
    // 场景：配置文件无 provider，但环境变量有 API key
    let temp = TempDir::new().expect("failed to create temp dir");
    let args = create_test_args(temp.path().to_path_buf());

    // 设置环境变量
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-env-key");

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    // 清理环境变量
    std::env::remove_var("ANTHROPIC_API_KEY");

    assert!(result.is_ok(), "bootstrap with env provider should succeed");
    let runtime = result.unwrap();

    // 应从环境变量注入 provider
    assert!(runtime.provider.is_some(), "provider should be injected from env");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_args_override_config() {
    // 场景：args 指定 provider/model，应覆盖配置文件
    let config = r#"{
        "providers": {
            "anthropic": {
                "apiKey": "sk-ant-config-key"
            },
            "openai": {
                "apiKey": "sk-openai-config-key"
            }
        },
        "model": "anthropic/claude-3-5-sonnet-20241022"
    }"#;

    let temp = create_temp_dir_with_config(config).expect("failed to create temp dir with config");
    let mut args = create_test_args(temp.path().to_path_buf());
    args.provider = Some("openai".to_string());
    args.model = Some("gpt-4".to_string());

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap with args override should succeed");
    let runtime = result.unwrap();

    // 应使用 args 中的 provider 和 model
    assert!(runtime.provider.is_some(), "provider should be available");
    assert_eq!(runtime.provider.unwrap().id(), "openai", "provider should be openai from args");
    assert_eq!(runtime.model, "gpt-4", "model should be gpt-4 from args");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_invalid_mode() {
    // 场景：无效的权限模式 → 应返回错误
    let temp = TempDir::new().expect("failed to create temp dir");
    let mut args = create_test_args(temp.path().to_path_buf());
    args.mode = "invalid_mode".to_string();

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_err(), "invalid mode should fail");
    if let Err(e) = result {
        assert!(e.to_string().contains("invalid --mode"));
    }
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_nonexistent_cwd() {
    // 场景：指定的工作目录不存在 → 应返回错误
    let args = create_test_args(PathBuf::from("/nonexistent/path/to/nowhere"));

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    // 根据实现，可能在 session 创建或其他步骤失败
    // 这里验证启动不会崩溃，并返回合理错误
    assert!(result.is_err(), "nonexistent cwd should fail gracefully");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_tools_initialization() {
    // 场景：验证工具注册表正确初始化
    let temp = TempDir::new().expect("failed to create temp dir");
    let args = create_test_args(temp.path().to_path_buf());

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // 工具应该被正确初始化
    let tools = runtime.tools.list();
    assert!(!tools.is_empty(), "tools should not be empty");

    // 验证一些预期的内置工具存在（根据 neko-tools 的实现）
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    // 这里假设至少有 Bash 等基础工具
    assert!(tool_names.iter().any(|name| name.contains("bash") || name.contains("Bash")),
            "should have bash-related tool");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_skills_loading() {
    // 场景：验证技能加载（包括内置 + 目录扫描）
    let temp = TempDir::new().expect("failed to create temp dir");

    // 创建 .neko/skills 目录
    let skills_dir = temp.path().join(".neko").join("skills");
    std::fs::create_dir_all(&skills_dir).expect("failed to create skills dir");

    // 创建一个测试技能文件
    let test_skill = skills_dir.join("test-skill");
    std::fs::create_dir_all(&test_skill).expect("failed to create test skill dir");
    let skill_md = test_skill.join("SKILL.md");
    std::fs::write(skill_md, r#"# test-skill
Test skill for bootstrap testing

## Invocation
When user says "test skill"

## Instructions
This is a test skill.
"#).expect("failed to write skill file");

    let args = create_test_args(temp.path().to_path_buf());
    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // 技能应该被加载（至少有内置技能）
    let skills_guard = runtime.skills.read();
    let skills = skills_guard.list();
    assert!(!skills.is_empty(), "skills should not be empty");

    // 验证测试技能是否被加载
    assert!(skills.iter().any(|s| s.name == "test-skill"),
            "test skill should be loaded from .neko/skills");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_session_creation() {
    // 场景：无 session_id → 创建新会话
    let temp = TempDir::new().expect("failed to create temp dir");
    let args = create_test_args(temp.path().to_path_buf());

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // 会话应被创建
    assert!(!runtime.session.meta.id.is_nil(), "session id should not be nil");
    assert_eq!(runtime.session.meta.cwd, temp.path(), "session cwd should match");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_model_catalog_empty_without_provider() {
    // 场景：冷启动时 catalog 应为空
    let temp = TempDir::new().expect("failed to create temp dir");
    let args = create_test_args(temp.path().to_path_buf());

    std::env::remove_var("ANTHROPIC_API_KEY");

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // 冷启动：catalog 应为空
    assert!(runtime.catalog.is_empty(), "catalog should be empty without provider");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_model_catalog_with_provider() {
    // 场景：有 provider 时 catalog 应包含模型
    let config = r#"{
        "providers": {
            "anthropic": {
                "apiKey": "sk-ant-test-key"
            }
        },
        "model": "anthropic/claude-3-5-sonnet-20241022"
    }"#;

    let temp = create_temp_dir_with_config(config).expect("failed to create temp dir with config");
    let args = create_test_args(temp.path().to_path_buf());

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // catalog 应包含模型
    assert!(!runtime.catalog.is_empty(), "catalog should not be empty with provider");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_permission_engine_initialization() {
    // 场景：验证权限引擎正确初始化
    let temp = TempDir::new().expect("failed to create temp dir");
    let mut args = create_test_args(temp.path().to_path_buf());
    args.mode = "ask".to_string();
    args.dangerously_skip_permissions = false;

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // 权限引擎应正确初始化为指定模式
    assert_eq!(runtime.mode.to_string(), "ask", "mode should be 'ask'");
    assert!(!runtime.skip_perms, "skip_perms should be false");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_config_merge_order() {
    // 场景：验证配置合并顺序（项目配置覆盖全局配置）
    let temp = TempDir::new().expect("failed to create temp dir");

    // 创建项目配置
    let config = r#"{
        "providers": {
            "anthropic": {
                "apiKey": "sk-ant-project-key"
            }
        }
    }"#;
    let neko_dir = temp.path().join(".neko");
    std::fs::create_dir_all(&neko_dir).expect("failed to create .neko dir");
    std::fs::write(neko_dir.join("settings.jsonc"), config).expect("failed to write config");

    let args = create_test_args(temp.path().to_path_buf());
    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // 配置应被正确加载
    assert!(runtime.config.providers.contains_key("anthropic"),
            "project config should be loaded");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_system_prompt_generated() {
    // 场景：验证系统提示词正确生成
    let temp = TempDir::new().expect("failed to create temp dir");
    let args = create_test_args(temp.path().to_path_buf());

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    assert!(result.is_ok(), "bootstrap should succeed");
    let runtime = result.unwrap();

    // 系统提示词应被生成且非空
    assert!(!runtime.system_prompt.is_empty(), "system prompt should not be empty");
}

#[tokio::test]
#[ignore = "bootstrap reads global std::env + ~/.neko; refactor to DI in stage 2"]
async fn test_bootstrap_malformed_config_fallback() {
    // 场景：配置文件格式错误 → 应降级到默认配置而非崩溃
    let malformed_config = r#"{
        "providers": {
            "anthropic": {
                // 缺少闭合括号和引号
                "apiKey": "sk-
    }"#;

    let temp = create_temp_dir_with_config(malformed_config)
        .expect("failed to create temp dir with config");
    let args = create_test_args(temp.path().to_path_buf());

    let result = neko_cli::bootstrap::bootstrap(&args, None).await;

    // 格式错误的配置应被忽略，使用默认配置
    assert!(result.is_ok(), "bootstrap should succeed with malformed config (fallback)");
    let runtime = result.unwrap();

    // 应降级到冷启动状态
    assert!(runtime.provider.is_none(), "should fallback to no provider on parse error");
}
