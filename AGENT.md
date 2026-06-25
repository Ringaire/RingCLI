# AGENT.md

<!-- 中文部分 / Chinese Version -->

## 贡献时使用 Agent 发起的提交修改 PR

当使用 agent 发起提交修改并创建 Pull Request 时，请遵循以下规范：

- **分支命名规范**：请使用 `agent-{type}/**` 的分支命名格式来提交 PR（即 Pull Request）
  - 示例：`agent-feat/add-login`、`agent-fix/typo-in-readme`、`agent-docs/update-api`、`agent-refactor/auth-module`
  - 其中 `{type}` 表示提交类型，常见类型包括：
    - `feat`：新功能
    - `fix`：修复缺陷
    - `docs`：文档更新
    - `style`：代码格式调整
    - `refactor`：代码重构
    - `test`：测试相关
    - `chore`：构建/工具链等杂项

> 请确保所有由 agent 创建的分支均符合此命名规范，以便于追踪和管理 agent 发起的贡献。

---

<!-- 英文部分 / English Version -->

## Agent-Initiated Commit & Pull Request Contributions

When using an agent to make commits and create a Pull Request, please follow these conventions:

- **Branch Naming Convention**: Please use the `agent-{type}/**` branch naming format to submit PRs (i.e., Pull Requests)
  - Examples: `agent-feat/add-login`, `agent-fix/typo-in-readme`, `agent-docs/update-api`, `agent-refactor/auth-module`
  - Where `{type}` indicates the commit type, common types include:
    - `feat`: New feature
    - `fix`: Bug fix
    - `docs`: Documentation update
    - `style`: Code formatting
    - `refactor`: Code refactor
    - `test`: Testing
    - `chore`: Build/toolchain and misc

> Please ensure that all branches created by the agent comply with this naming convention, so as to facilitate tracking and managing agent-initiated contributions.
