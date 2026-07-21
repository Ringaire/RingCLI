# RingCLI

<div align="center">

**Terminal AI Coding Assistant — Open Source, Multi-Provider, Orchestable**

English | [简体中文](README.md)

(The project is in its initial stage; version stays 0.x.x before official release. Follows [Semantic Versioning 2.0](https://semver.org/))

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](https://github.com/Ringaire/RingCLI)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-orange.svg)](LICENSE)

An extensible terminal AI coding assistant supporting **34 LLM providers**, with 17 built-in tools

[Features](#features) • [Install](#install) • [Commands](#commands) • [Providers](#supported-providers) • [Config](#configuration-directory) • [Roadmap](#roadmap)

</div>

---

## Install

```bash
# Build from source (requires Rust 1.85+)
git clone https://github.com/Ringaire/RingCLI.git
cd RingCLI
cargo install --path crates/ring-cli --bin ring

# Or run directly
cargo run --release
```

### Configure Provider

```bash
# Option 1: Interactive wizard
ring
# Type /connect, select provider, enter API key

# Option 2: Quick connect
ring
# /connect anthropic sk-ant-xxx

# Option 3: Environment variables
export ANTHROPIC_API_KEY="sk-ant-xxx"
export OPENAI_API_KEY="sk-xxx"
export DEEPSEEK_API_KEY="sk-xxx"
```

Config file lives at `~/.ring/config/settings.jsonc` (comments supported).

---

## Features

- 🤖 **34 Providers** — Anthropic / OpenAI / Gemini / DeepSeek / Zhipu / Qwen / MiniMax / Xiaomi / HuggingFace / Cloudflare, etc. OpenAI OAuth2 login
- 🛠️ **17 Built-in Tools** — bash, file read/write/edit, search, web, LSP, TODO, session management
- 🎭 **Multi-Agent Orchestration** — sub-agent spawn + role-based model selection + view separation
- 🔐 **Five Permission Tiers** — Ask (read-only) → Edit (code) → Plan (planning) → Build (all) → Agent (autonomous)
- 🧠 **Seven Thinking Levels** — `/effort off|minimal|low|medium|high|xhigh|max`, dropdown + `Shift+Tab` cycle
- 📝 **Orchestrable Prompts** — SKILL.md supports `$1` `$@` `${N:-default}` `${@:N}` argument substitution
- 📂 **Project-level Config** — `.ring/` directory: SYSTEM.md / APPEND_SYSTEM.md / tool.json / mode / doc
- 📦 **Session Persistence** — JSONL storage + automatic context compaction
- 🔌 **MCP Compatible** — `tool.json` / `mcp_server.json` / `.mcp.json`, multi-path discovery
- 🧩 **Skill / Doc System** — SKILL.md (Markdown + frontmatter), compatible with `.agents/skills/` and `.ring/doc/`
- 🖥️ **TUI Interface** — Ratatui + custom Markdown renderer + width-aware wrapping

---

## Commands

Type `/` to see all:

| Command | Description |
|---------|-------------|
| `/connect` | Configure provider (wizard / quick connect / ChatGPT OAuth2) |
| `/logout` | Remove stored provider credentials (dropdown / `/logout <provider>`) |
| `/model` | Model picker (cross-provider grouping + search) |
| `/mode` | Permission mode picker (ask/edit/plan/build/agent) |
| `/effort` | Thinking level dropdown (off/minimal/low/medium/high/xhigh/max) |
| `/think` | Extended thinking (on/off [budget]) |
| `/thinking` | Fold/expand reasoning process |
| `/sessions` | Session management |
| `/compact` | Compact context |
| `/new` | New session |
| `/help` | Full command list |

### Keyboard Shortcuts

| Key | Description |
|-----|-------------|
| `↑` `↓` | Multi-line cursor / history (on empty input) |
| `Tab` | Accept completion / cycle permission mode |
| **`Shift+Tab`** | **Cycle thinking level** (off→minimal→low→medium→high→xhigh→max) |
| `Ctrl+T` | Toggle extended thinking |
| `Ctrl+O` | Fold/expand reasoning process |
| `Ctrl+↑` `Ctrl+↓` | Sub-agent view switch |
| `Alt+↑` `Alt+↓` | Chat scroll |
| `Alt+Enter` | Multi-line input |
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
| **HuggingFace / MiniMax / MiniMax-CN / Qwen / Qwen-CN** | OpenAI-compatible |
| **Xiaomi / Xiaomi-AMS / Xiaomi-SGP / ZAI / ZAI-CN / Ant-Ling** | OpenAI-compatible |
| **Cloudflare-Workers / Cloudflare-Gateway / Vercel-Gateway / OpenCode** | OpenAI-compatible |
| **Ollama / LM Studio** | Local models (no key needed) |
| **Custom** | Any OpenAI-compatible API |

---

## Configuration Directory

### Global (`~/.ring/`)

```
~/.ring/
├── config/
│   ├── settings.jsonc       # Main config (comments supported)
│   ├── providers.json       # Provider overrides
│   ├── tool.json            # MCP tools (new unified format)
│   ├── mcp_server.json      # MCP tools (legacy compat)
│   ├── .mcp.json            # Claude Code compat
│   ├── SYSTEM.md            # Custom system prompt (replaces default)
│   └── APPEND_SYSTEM.md     # Append to system prompt
├── skills/                  # Global skills
├── doc/                     # Global docs (skill new name)
├── prompts/                 # Prompt templates
├── mode/                    # Mode definitions
└── sessions/                # Session storage (JSONL)
```

### Project-level (`./.ring/`, higher priority than global)

```
./.ring/
├── settings.jsonc           # Project config
├── tool.json                # Project MCP tools
├── mcp_server.json          # Project MCP tools (legacy)
├── .mcp.json                # Claude Code compat
├── SYSTEM.md                # Project custom system prompt
├── APPEND_SYSTEM.md         # Project append system prompt
├── skills/                  # Project skills
├── doc/                     # Project docs
├── mode/                    # Project mode definitions
│   └── {mode}.md            # Mode prompts
└── workflow.json            # Model orchestration (planned)
```

### Context File Discovery (priority high → low)

RingCLI auto-discovers and injects these files into the system prompt:

| File | Description | Discovery |
|------|-------------|-----------|
| `AGENTS.md` | Project instructions | Traverse up from cwd to git root |
| `CLAUDE.md` | Pi / Claude Code compat | Traverse up from cwd to git root |
| `.ring/SYSTEM.md` | Custom system prompt | Project > global |
| `.ring/APPEND_SYSTEM.md` | Append to system prompt end | Project > global |

### MCP Tool Config Discovery (priority high → low)

Merged in this order (near overrides far):

1. `cwd/tool.json` — Working dir new format
2. `cwd/mcp_server.json` — Working dir legacy format
3. `cwd/.mcp.json` — Claude Code compat (traverse up)
4. `cwd/.ring/{tool,mcp_server,.mcp}.json` — Project `.ring/`
5. `~/.ring/config/{tool,mcp_server,.mcp}.json` — Global

---

## Roadmap

- [x] Multi-provider + OAuth2 (34 providers)
- [x] 17 built-in tools + MCP compat (multi-path discovery)
- [x] Multi-agent orchestration + view separation
- [x] Five permission tiers + seven thinking levels
- [x] Skill / Doc system (SKILL.md + argument substitution)
- [x] Project-level `.ring/` config + SYSTEM.md / APPEND_SYSTEM.md
- [x] Custom Markdown renderer
- [ ] `.ring/mode/{mode}.md` mode system
- [ ] `.ring/workflow.json` model orchestration
- [ ] API Key rotation
- [ ] File change detection (Snapshot)
- [ ] VSCode extension

---

## License

[AGPL-3.0](LICENSE)

---

<div align="center">

Made with ❤️ by Ringaire玲汐

</div>
