# NekoCLI

<div align="center">

**终端 AI 编程助手 — 开源、多 Provider、可编排**

[English](README_EN.md) | 简体中文

(项目处于初始阶段，版本号在正式发布前保持 0.x.x。遵循 [语义化版本 2.0](https://semver.org/lang/zh-CN/))

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](https://github.com/Ringaire/NekoCLI)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-orange.svg)](LICENSE)

一个可扩展的终端 AI 编程助手，支持 20+ LLM 服务商，内置 17 个开发工具

[功能](#功能) • [安装](#安装) • [命令](#命令) • [Provider](#支持的-provider) • [路线图](#路线图)

</div>

---

## 安装

```bash
# 从源码构建（需要 Rust 1.85+）
git clone https://github.com/Ringaire/NekoCLI.git
cd NekoCLI
cargo install --path crates/neko-cli --bin neko

# 或直接运行
cargo run --release
```

### 配置 Provider

```bash
# 方式一：交互式向导
neko
# 输入 /connect，选择 provider，输入 API key

# 方式二：快速连接
neko
# /connect anthropic sk-ant-xxx

# 方式三：环境变量
export ANTHROPIC_API_KEY="sk-ant-xxx"
export OPENAI_API_KEY="sk-xxx"
export DEEPSEEK_API_KEY="sk-xxx"
```

配置文件位于 `~/.config/neko/settings.jsonc`（支持注释）。

---

## 功能

- 🤖 **20+ Provider** — Anthropic / OpenAI / Gemini / DeepSeek / 智谱 / Groq / Ollama 等，OpenAI OAuth2 登录
- 🛠️ **17 个内置工具** — bash、文件读写编辑、搜索、Web、LSP、TODO、会话管理
- 🎭 **多 Agent 编排** — 子 agent 派生 + role-based 选模 + 视图分离
- 🔐 **权限四档** — Ask（只读）→ Edit（改码）→ Build（全开）→ Agent（自主）
- 🧠 **思考程度** — `/effort low|medium|high|max`，`/thinking` 折叠推理过程
- 📦 **会话持久化** — JSONL 存储 + 上下文自动压缩
- 🔌 **MCP 兼容** — 共用 Claude Code 的 `.mcp.json`
- 🧩 **Skill 系统** — SKILL.md（Markdown + frontmatter），检索 `.agents/skills/`
- 🖥️ **TUI 终端界面** — Ratatui + 自实现 Markdown 渲染 + 宽度感知换行

---

## 命令

输入 `/` 查看：

| 命令 | 说明 |
|------|------|
| `/connect` | 配置 provider（向导 / 快速连接 / ChatGPT OAuth2） |
| `/model` | 模型选择器（跨 provider 分组 + 搜索） |
| `/mode` | 权限模式选择器（ask/edit/plan/build/agent） |
| `/effort` | 思考程度（low/medium/high/max） |
| `/think` | Extended thinking（on/off [budget]） |
| `/thinking` | 折叠/展开推理过程 |
| `/sessions` | 会话管理 |
| `/compact` | 压缩上下文 |
| `/new` | 新建会话 |
| `/help` | 完整命令列表 |

### 快捷键

| 键 | 说明 |
|----|------|
| `↑` `↓` | 多行光标移动 / 历史（空输入时） |
| `Ctrl+↑` `Ctrl+↓` | 子 agent 视图切换 |
| `Tab` | 补全接受 / 权限模式切换 |
| `Alt+↑` `Alt+↓` | 聊天滚动 |
| `Alt+Enter` | 多行输入 |
| `@path` | 文件引用 |
| 粘贴 `file://` | 自动转 `@path` |

---

## 支持的 Provider

| Provider | 类型 |
|----------|------|
| **Anthropic** | 原生 SDK + OAuth2 |
| **OpenAI** | 原生 SDK + ChatGPT OAuth2 |
| **Google Gemini** | 原生 SDK |
| **DeepSeek / Groq / Mistral / Together / OpenRouter / xAI** | OpenAI 兼容 |
| **Moonshot / SiliconFlow / Zhipu / Baidu / NVIDIA** | OpenAI 兼容 |
| **Cerebras / DeepInfra / Fireworks / Perplexity / Cohere** | OpenAI 兼容 |
| **Ollama / LM Studio** | 本地模型（无需 key） |
| **Custom** | 任意 OpenAI 兼容 API |

---

## 路线图

- [x] 多 Provider + OAuth2
- [x] 17 个内置工具 + MCP 兼容
- [x] 多 Agent 编排 + 视图分离
- [x] 权限四档 + 思考程度
- [x] Skill 系统（SKILL.md）
- [x] 自实现 Markdown 渲染
- [ ] API Key 轮询
- [ ] 文件变更检测（Snapshot）
- [ ] VSCode 扩展

---

## 许可证

[AGPL-3.0](LICENSE)

---

<div align="center">

Made with ❤️ by Ringaire玲汐

</div>
