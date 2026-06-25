# NekoCLI

<div align="center">

**终端 AI 编程助手 — 多 Provider、多工具、多模型编排**

(目前项目正在处于初始阶段，因此版本号在发布正式版本之前一直保持大版本为 0.x.x。本项目遵循 [语义化版本 2.0](https://semver.org/lang/zh-CN/) 规范。有问题请及时反馈至 Issue)

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](https://github.com/Ringaire/NekoCLI)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-orange.svg)](LICENSE)

一个功能强大、可扩展的终端 AI 编程助手，支持 20+ LLM 服务商，内置 15 个开发工具

[功能特性](#功能特性) • [快速开始](#快速开始) • [架构设计](#架构设计) • [支持的提供商](#支持的-llm-提供商) • [路线图](#路线图)

</div>

---

## 功能特性

### 核心功能

- 🤖 **多 Provider 支持** — Anthropic、OpenAI、Google Gemini、DeepSeek、Groq 等 20+ 服务商
- 🛠️ **15 个内置工具** — bash、文件操作、搜索、Web、LSP、TODO、Token 计数、会话管理
- 🖥️ **TUI 终端界面** — 基于 Ratatui，支持 Markdown 渲染、流式输出、推理展示
- 🧠 **Extended Thinking** — 支持 Anthropic、OpenAI o-series、DeepSeek 深度思考
- 🎭 **Orchestrator 模式** — 多模型子 Agent 编排，自动选择模型角色
- 🔐 **权限系统** — build/edit/ask 三种模式，细粒度工具权限控制
- 📦 **会话持久化** — JSONL 存储，支持历史恢复和压缩
- 🔌 **MCP 协议** — Model Context Protocol 集成
- 🧩 **插件系统** — 通过 npm 包扩展功能

### 工具列表

| 工具 | 说明 |
|------|------|
| `bash` | 执行 Shell 命令 |
| `read_file` | 读取文件（支持行范围切片） |
| `edit_file` | 精确替换文件内容 |
| `write_file` | 写入文件（自动创建目录） |
| `tree` | 显示目录树（自动排除构建产物） |
| `glob` | 按模式搜索文件 |
| `grep` | 用 ripgrep 搜索文件内容 |
| `web_fetch` | 抓取网页内容 |
| `web_search` | DuckDuckGo 搜索 |
| `lsp_diagnostics` | TypeScript 类型检查 |
| `lsp_refs` | 查找符号引用 |
| `todo` | 会话级 TODO 管理 |
| `token_count` | Token 估算 |
| `list_sessions` | 列出历史会话 |
| `search_sessions` | 搜索历史会话 |

---

## 快速开始

### 环境要求

- Rust 1.85+
- 网络连接（用于 LLM 服务调用）

### 安装

1. 克隆仓库
```bash
git clone https://github.com/Ringaire/NekoCLI.git
cd NekoCLI
```

2. 构建
```bash
cargo build --release
```

3. 配置 Provider
```bash
# 设置 API Key（选择一个或多个）
export ANTHROPIC_API_KEY="sk-ant-xxx"
export OPENAI_API_KEY="sk-xxx"
export DEEPSEEK_API_KEY="sk-xxx"
```

4. 安装到系统（推荐）
```bash
cargo install --path . --bin neko
neko
```

5. 或直接运行（不安装）
```bash
cargo run --release
```

### 配置文件

配置文件位于 `~/.config/neko/settings.jsonc`（XDG 规范），支持 JSONC 格式（可添加注释）。

```jsonc
{
  // 默认模型（provider/model-id 格式）
  "model": "anthropic/claude-sonnet-4-6",

  // Provider 配置
  "providers": {
    "anthropic": {
      "apiKey": "sk-ant-xxx"  // 或使用环境变量 ANTHROPIC_API_KEY
    },
    "deepseek": {
      "apiKey": "sk-xxx"
    }
  }
}
```

Provider 定义位于 `~/.config/neko/providers.json`（全局）或 `.neko/providers.json`（项目级）。

---

## 命令列表

启动后在 TUI 中输入 `/` 查看所有命令：

| 命令 | 说明 |
|------|------|
| `/help` | 显示完整命令列表 |
| `/model [id]` | Show cached models, fuzzy search, or switch |
| `/model refresh` | Re-fetch model list from provider API |
| `/model reload` | Reload model config + refresh cache |
| `/connect [provider] [key]` | Configure provider + cache models |
| `/sessions [id]` | 列出或加载历史会话 |
| `/new` | 新建会话 |
| `/compact` | 压缩历史（摘要替换） |
| `/think [on\|off] [budget]` | 切换 Extended Thinking |
| `/orchestrate` | 切换 Orchestrator 模式 |
| `/review [args]` | 代码审查（默认 git diff） |
| `/diff` | 显示 git diff |
| `/init` | 生成 AGENTS.md |
| `/allow <tool>` | 允许工具 |
| `/deny <tool>` | 禁止工具 |
| `/plugin install <pkg>` | 安装插件 |
| `/reload` | 热重载配置、MCP、技能 |
| `/exit` | 退出 |

### 快捷键

| 快捷键 | 说明 |
|--------|------|
| `Tab` | 切换模式 (build → edit → ask) |
| `↑ / ↓` | 浏览输入历史 |
| `Alt+↑ / Alt+↓` | 滚动聊天记录（行级） |
| `Page Up / Page Down` | 滚动聊天记录（页级） |
| `Ctrl+A / Ctrl+E` | 行首 / 行尾 |
| `Ctrl+C` | 清空输入或退出 |
| `@file.ts` | 附加文件或目录 |

---

## 架构设计

```
crates/
├── neko-cli/        ── 命令行入口 & TUI 界面 (Ratatui)
│   ├── src/agent/    ── Agent 编排、Turn 循环、模型发现
│   ├── src/tui/      ── Ratatui 组件 (App, MessageList, PromptInput)
│   └── src/repl/     ── REPL 命令处理、模式切换
│
├── neko-core/       ── 核心引擎
│   ├── agent/        ── 模型选择器、角色分类
│   ├── config/       ── 配置加载、路径管理
│   ├── events/       ── 事件总线（Agent, Tool, Session, Context）
│   ├── permissions/  ── 权限引擎（build/edit/ask 模式）
│   ├── session/      ── 会话存储（JSONL）、记忆管理
│   └── tools/        ── 工具接口定义、注册表
│
├── neko-providers/  ── LLM Provider 适配器
│   ├── anthropic/    ── Anthropic (Claude)
│   ├── openai/       ── OpenAI (GPT, o-series)
│   ├── gemini/       ── Google Gemini
│   └── compatible/   ── DeepSeek, Groq, SiliconFlow 等
│
├── neko-tools/      ── 15 个内置工具实现
│   ├── bash/         ── Shell 命令执行
│   ├── file/         ── 读/写/编辑/树
│   ├── search/       ── glob/grep
│   ├── web/          ── fetch/search
│   ├── lsp/          ── TypeScript 诊断/引用
│   ├── todo/         ── TODO 管理
│   ├── tokens/       ── Token 估算
│   └── sessions/     ── 会话列表/搜索
│
├── neko-mcp/        ── MCP 协议桥接
├── neko-skills/     ── 技能注册系统
```

### 事件系统

统一的事件命名，屏蔽 Provider 差异：

| 事件 | 说明 |
|------|------|
| `agent:thinking` | Agent 开始思考 |
| `agent:reasoning` | 推理 token 流式输出 |
| `agent:reasoning_done` | 推理完成 |
| `agent:text` | 文本 token 流式输出 |
| `agent:text_done` | 文本输出完成 |
| `agent:tool_call` | 工具调用 |
| `agent:error` | 错误 |
| `agent:done` | Turn 结束 |
| `tool:start` | 工具开始执行 |
| `tool:end` | 工具执行完成 |
| `session:start` | 会话开始 |
| `session:end` | 会话结束 |
| `context:update` | 上下文更新 |
| `context:truncate` | 上下文截断 |
| `context:summary` | 上下文摘要 |

---

## 支持的 LLM 提供商

| 提供商 | 类型 | 模型示例 | 说明 |
|--------|------|----------|------|
| **Anthropic** | 原生 SDK | claude-sonnet-4-6, claude-opus-4-7 | Extended Thinking |
| **OpenAI** | 原生 SDK | gpt-4o, o3, o4-mini | Reasoning (o-series) |
| **Google Gemini** | 原生 SDK | gemini-2.0-flash, gemini-pro | |
| **DeepSeek** | OpenAI 兼容 | deepseek-chat, deepseek-r1 | 深度思考 |
| **Groq** | OpenAI 兼容 | llama-3.3-70b-versatile | 高速推理 |
| **SiliconFlow** | OpenAI 兼容 | Qwen/Qwen2.5-72B-Instruct | 国内服务 |
| **OpenRouter** | OpenAI 兼容 | anthropic/claude-sonnet-4-6 | 聚合路由 |
| **Mistral** | OpenAI 兼容 | mistral-large-latest | |
| **Together AI** | OpenAI 兼容 | Llama-3-70b-chat-hf | |
| **Moonshot** | OpenAI 兼容 | moonshot-v1-8k | 国内服务 |
| **Zhipu AI** | OpenAI 兼容 | glm-4 | 国内服务 |
| **Baidu ERNIE** | OpenAI 兼容 | ernie-4.0-turbo-8k | 国内服务 |
| **xAI** | OpenAI 兼容 | grok-3 | |
| **Cerebras** | OpenAI 兼容 | llama-3.3-70b | 高速推理 |
| **Perplexity** | OpenAI 兼容 | sonar | 搜索增强 |
| **Ollama** | OpenAI 兼容 | llama3.2, qwen2.5 | 本地模型 |
| **LM Studio** | OpenAI 兼容 | local-model | 本地模型 |

> 还支持任何 OpenAI 兼容 API，只需配置 `baseUrl`。

---

## 路线图

- [x] 多 Provider 支持（20+）
- [x] 15 个内置工具
- [x] TUI 终端界面（Ratatui）
- [x] Extended Thinking / Reasoning
- [x] Orchestrator 多模型编排
- [x] 会话持久化（JSONL）
- [x] MCP 协议集成
- [x] 插件系统
- [ ] 权限系统 — 真实的 ask 确认流程
- [ ] Session Picker — 交互式会话选择
- [ ] 多行输入 + 粘贴检测
- [ ] 文件变更检测（Snapshot 系统）
- [ ] 虚拟滚动 + 工具输出折叠
- [ ] 全局快捷键系统（Keymap）
- [ ] Dialog / Modal 统一管理
- [ ] API Key 轮训
- [ ] 上下文自动压缩
- [ ] VSCode 扩展

---

## 许可证

本项目采用 AGPL-3.0 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情

---

## 致谢

- 参考项目：
    - [OpenCode](https://github.com/nicepkg/opencode) — TUI 架构、Permission 系统、Dialog 设计
    - [Claude Code](https://github.com/anthropics/claude-code) — Paste 处理、Permission 交互、工具系统
    - [NekoBot](https://github.com/Ringaire/NekoBot) — 框架设计、插件系统
- 所有贡献者

---

<div align="center">

**如果这个项目对你有帮助，请给个 Star ⭐**

Made with ❤️ by Ringaire玲汐
</div>
