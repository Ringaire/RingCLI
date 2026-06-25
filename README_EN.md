# NekoCLI

<div align="center">

**Terminal AI coding agent — open source, multi-provider, orchestrable**

English | [简体中文](README.md)

(Project is in early stage, version stays 0.x.x until stable release. Follows [SemVer 2.0](https://semver.org/))

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](https://github.com/Ringaire/NekoCLI)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-orange.svg)](LICENSE)

An extensible terminal AI coding agent, supporting 20+ LLM providers with 17 built-in tools

[Features](#features) • [Install](#install) • [Commands](#commands) • [Providers](#supported-providers) • [Roadmap](#roadmap)

</div>

---

## Install

```bash
# Build from source (requires Rust 1.85+)
git clone https://github.com/Ringaire/NekoCLI.git
cd NekoCLI
cargo install --path crates/neko-cli --bin neko

# Or run directly
cargo run --release
```

### Configure Provider

```bash
# Option 1: Interactive wizard
neko
# Type /connect, select provider, enter API key

# Option 2: Quick connect
neko
# /connect anthropic sk-ant-xxx

# Option 3: Environment variables
export ANTHROPIC_API_KEY="sk-ant-xxx"
export OPENAI_API_KEY="sk-xxx"
export DEEPSEEK_API_KEY="sk-xxx"
```

Config file at `~/.config/neko/settings.jsonc` (supports comments).

---

## Features

- 🤖 **20+ Providers** — Anthropic / OpenAI / Gemini / DeepSeek / Zhipu / Groq / Ollama, OpenAI OAuth2 login
- 🛠️ **17 built-in tools** — bash, file read/write/edit, search, web, LSP, TODO, sessions
- 🎭 **Multi-agent orchestration** — sub-agent spawn + role-based model selection + view isolation
- 🔐 **Permission tiers** — Ask (read-only) → Edit → Build → Agent (autonomous)
- 🧠 **Reasoning effort** — `/effort low|medium|high|max`, `/thinking` to fold reasoning
- 📦 **Session persistence** — JSONL storage + auto context compaction
- 🔌 **MCP compatible** — shares Claude Code's `.mcp.json`
- 🧩 **Skill system** — SKILL.md (Markdown + frontmatter), discovers `.agents/skills/`
- 🖥️ **TUI** — Ratatui + custom Markdown renderer with width-aware wrapping

---

## Commands

Type `/` to see all:

| Command | Description |
|---------|-------------|
| `/connect` | Configure provider (wizard / quick / ChatGPT OAuth2) |
| `/model` | Model picker (cross-provider, searchable) |
| `/mode` | Permission mode picker (ask/edit/plan/build/agent) |
| `/effort` | Reasoning effort (low/medium/high/max) |
| `/think` | Extended thinking (on/off [budget]) |
| `/thinking` | Toggle reasoning display |
| `/sessions` | Session management |
| `/compact` | Compact context |
| `/new` | New session |
| `/help` | Full command list |

### Keybindings

| Key | Action |
|-----|--------|
| `↑` `↓` | Multiline cursor / history (when empty) |
| `Ctrl+↑` `Ctrl+↓` | Sub-agent view switch |
| `Tab` | Accept suggestion / cycle mode |
| `Alt+↑` `Alt+↓` | Scroll chat |
| `Alt+Enter` | Newline |
| `@path` | File reference |
| Paste `file://` | Auto-convert to `@path` |

---

## Supported Providers

| Provider | Type |
|----------|------|
| **Anthropic** | Native SDK + OAuth2 |
| **OpenAI** | Native SDK + ChatGPT OAuth2 |
| **Google Gemini** | Native SDK |
| **DeepSeek / Groq / Mistral / Together / OpenRouter / xAI** | OpenAI-compatible |
| **Moonshot / SiliconFlow / Zhipu / Baidu / NVIDIA** | OpenAI-compatible |
| **Cerebras / DeepInfra / Fireworks / Perplexity / Cohere** | OpenAI-compatible |
| **Ollama / LM Studio** | Local (no key needed) |
| **Custom** | Any OpenAI-compatible API |

---

## Roadmap

- [x] Multi-provider + OAuth2
- [x] 17 built-in tools + MCP compat
- [x] Multi-agent orchestration + view isolation
- [x] Permission tiers + reasoning effort
- [x] Skill system (SKILL.md)
- [x] Custom Markdown renderer
- [ ] API key rotation
- [ ] File change detection (Snapshot)
- [ ] VSCode extension

---

## License

[AGPL-3.0](LICENSE)

---

<div align="center">

Made with ❤️ by Ringaire玲汐

</div>
