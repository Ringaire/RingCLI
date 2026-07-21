# 贡献指南

[English](CONTRIBUTING_EN.md) | 简体中文

感谢你对 RingCLI 的兴趣！本文档说明如何参与开发。

---

## 开发环境

- Rust 1.85+
- Git
- 一个 LLM API key（用于功能测试）

```bash
git clone https://github.com/Ringaire/RingCLI.git
cd RingCLI
cargo build
```

---

## 分支命名

从 `develop` 拉取分支，使用前缀/描述格式：

| 前缀 | 用途 | 示例 |
|------|------|------|
| `feat/` | 新功能或新特性 | `feat/user-profile-settings` |
| `fix/` | 修复 Bug | `fix/session-timeout` |
| `docs/` | 仅修改文档、注释 | `docs/api-usage-guide` |
| `refactor/` | 代码重构，不改变功能 | `refactor/payment-service` |
| `perf/` | 性能优化 | `perf/image-lazy-load` |
| `style/` | 代码格式、拼写等不影响逻辑的改动 | `style/fix-typo` |
| `chore/` | 构建流程、工具链、依赖更新等杂项 | `chore/update-dependencies` |
| `ci/` | 修改 CI/CD 配置或脚本 | `ci/fix-build-pipeline` |

```bash
git checkout develop
git pull origin develop
git checkout -b feat/your-feature
```

---

## 提交信息

使用 [Conventional Commits](https://www.conventionalcommits.org/) 格式：

```
<type>(<scope>): <description>

[optional body]
```

**Type**：

| Type | 用途 |
|------|------|
| `feat` | 新功能 |
| `fix` | Bug 修复 |
| `refactor` | 代码重构，无行为变化 |
| `docs` | 文档 |
| `test` | 测试 |
| `chore` | 构建、CI、依赖、工具 |
| `perf` | 性能优化 |
| `style` | 格式、拼写 |

**示例**：

```
feat(tui): add mode picker with scroll selection
fix(provider): max_tokens sent as context window size
docs(readme): remove architecture section, simplify
refactor(engine): move executor from ring-cli to ring-engine
```

- **scope** 可选，建议多模块项目使用（如 `tui`、`provider`、`engine`、`mcp`）
- **description**：祈使句，小写，无句号
- **body**：解释 *为什么*，不是 *什么*

---

## 代码规范

### Rust

- 使用 `cargo fmt` 格式化
- 使用 `cargo clippy` 检查，无 warning
- 优先 `Option`/`Result`，避免 `unwrap()`（测试除外）
- 公共 API 加文档注释 `///`
- 模块内代码不超过 500 行，超了就拆

### 测试

```bash
cargo test
```

- 核心逻辑写单元测试
- 测试名描述行为：`test_parse_invalid_date_returns_default`
- 不需要给 trivial getter/setter 写测试

### 新增 Provider

1. 在 `crates/ring-providers/src/providers/added/{name}/` 下创建 `mod.rs`
2. 定义 `catalog_entry()` 函数
3. 在 `added/mod.rs` 声明 `pub mod {name};`
4. 在 `catalog.rs` 的 `defaults()` 注册

### 新增工具

1. 在 `crates/ring-tools/src/tools/` 下创建 `{name}.rs`
2. 实现 `Tool` trait
3. 在 `register.rs` 注册

---

## PR 流程

1. 从 `develop` 拉分支
2. 开发 + 测试通过（`cargo test` + `cargo clippy`）
3. 如果涉及 UI 变化，附截图
4. 提交 PR 到 `develop` 分支
5. 等待 review

### PR 标题

同 Conventional Commits：

```
feat(tui): add effort command for reasoning control
```

---

## 项目结构速览

```
crates/
├── ring-core/       类型、事件、权限、会话、配置
├── ring-providers/  LLM 适配（Anthropic/OpenAI/Gemini/Compatible + OAuth2）
├── ring-tools/      17 个内置工具
├── ring-mcp/        MCP 协议客户端
├── ring-skills/     Skill 加载（SKILL.md）
├── ring-engine/     Agent 执行循环、编排
└── ring-cli/        TUI、REPL、命令系统
```

---

## 行为准则

- 保持尊重和友善
- 用中文或英文交流
- 如果不确定，先开 Issue 讨论

---

<div align="center">

提问请开 Issue · 讨论请开 Discussion

</div>
