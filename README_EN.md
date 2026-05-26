# NekoCode

<div align="center">

**Terminal AI Coding Assistant — Multi-Provider, Multi-Tool, Multi-Model Orchestration**

( The project is currently in its initial stage, so the version number will remain at 0.x.x until the official release. This project follows [Semantic Versioning 2.0](https://semver.org/). Please report issues via GitHub Issues )

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](https://github.com/Carillen/NekoCode)
[![TypeScript](https://img.shields.io/badge/typescript-5.9+-blue.svg)](https://www.typescriptlang.org/)
[![Node](https://img.shields.io/badge/node.js-20+-green.svg)](https://nodejs.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-orange.svg)](LICENSE)

A powerful, extensible terminal AI coding assistant with 20+ LLM providers and 15 built-in developer tools

[Features](#features) • [Quick Start](#quick-start) • [Architecture](#architecture) • [Providers](#supported-llm-providers) • [Roadmap](#roadmap) • [中文](README.md)

</div>

---

## Features

### Core

- 🤖 **Multi-Provider Support** — Anthropic, OpenAI, Google Gemini, DeepSeek, Groq and 20+ more
- 🛠️ **15 Built-in Tools** — bash, file operations, search, web, LSP, TODO, token counting, session management
- 🖥️ **TUI Interface** — React/ink based, markdown rendering, streaming output, reasoning display
- 🧠 **Extended Thinking** — Anthropic, OpenAI o-series, DeepSeek deep thinking support
- 🎭 **Orchestrator Mode** — Multi-model sub-agent delegation with automatic role selection
- 🔐 **Permission System** — build/edit/ask modes with fine-grained tool access control
- 📦 **Session Persistence** — JSONL storage with history restore and compaction
- 🔌 **MCP Protocol** — Model Context Protocol integration
- 🧩 **Plugin System** — Extend via npm packages

### Tools

| Tool | Description |
|------|-------------|
| `bash` | Execute shell commands |
| `read_file` | Read files (with line range slicing) |
| `edit_file` | Precise string replacement |
| `write_file` | Write files (auto-create directories) |
| `tree` | Display directory tree (auto-exclude build artifacts) |
| `glob` | Search files by pattern |
| `grep` | Search file contents with ripgrep |
| `web_fetch` | Fetch web page content |
| `web_search` | DuckDuckGo search |
| `lsp_diagnostics` | TypeScript type checking |
| `lsp_refs` | Find symbol references |
| `todo` | Session-level TODO management |
| `token_count` | Token estimation |
| `list_sessions` | List conversation history |
| `search_sessions` | Search conversation history |

---

## Quick Start

### Requirements

- Node.js 20+
- pnpm (recommended)
- Network connection (for LLM API calls)

### Installation

1. Clone the repository
```bash
git clone https://github.com/Carillen/NekoCode.git
cd NekoCode
```

2. Install dependencies
```bash
pnpm install
```

3. Build
```bash
pnpm build
```

4. Configure providers
```bash
# Set API keys (choose one or more)
export ANTHROPIC_API_KEY="sk-ant-xxx"
export OPENAI_API_KEY="sk-xxx"
export DEEPSEEK_API_KEY="sk-xxx"
```

5. Run
```bash
# Development mode (hot reload)
pnpm dev

# Production mode
pnpm start
```

### Configuration

Config file is located at `~/.config/nekocode/settings.json` (XDG spec), supports JSONC format (comments allowed).

```jsonc
{
  // Default model (provider/model-id format)
  "model": "anthropic/claude-sonnet-4-6",

  // Provider configuration
  "providers": {
    "anthropic": {
      "apiKey": "sk-ant-xxx"  // Or use env var ANTHROPIC_API_KEY
    },
    "deepseek": {
      "apiKey": "sk-xxx"
    }
  }
}
```

---

## Commands

Type `/` in the TUI to see all commands:

| Command | Description |
|---------|-------------|
| `/help` | Show full command list |
| `/model [id]` | Show cached models, fuzzy search, or switch |
| `/model reload` | Re-fetch model list from provider API |
| `/connect [provider] [key]` | Configure provider + cache models |
| `/sessions [id]` | List or load session history |
| `/new` | New session |
| `/compact` | Compress history (summary replace) |
| `/think [on\|off] [budget]` | Toggle Extended Thinking |
| `/orchestrate` | Toggle Orchestrator mode |
| `/review [args]` | Code review (default: git diff) |
| `/diff` | Show git diff |
| `/init` | Generate AGENTS.md |
| `/allow <tool>` | Allow tool |
| `/deny <tool>` | Deny tool |
| `/plugin install <pkg>` | Install plugin |
| `/reload` | Hot reload config, MCP, skills |
| `/exit` | Exit |

### Keybindings

| Key | Action |
|-----|--------|
| `Tab` | Cycle mode (build → edit → ask) |
| `↑ / ↓` | Browse input history |
| `Ctrl+A / Ctrl+E` | Line start / end |
| `Ctrl+C` | Clear input or exit |
| `@file.ts` | Attach file or directory |

---

## Architecture

```
packages/
├── cli/            ── CLI entry & TUI
│   ├── src/agent/    ── Agent orchestration, Turn loop, model discovery
│   ├── src/tui/      ── React/ink components (App, MessageList, PromptInput)
│   ├── src/repl/     ── REPL command handling, mode switching
│   └── src/input/    ── Input parsing, command completion, @mentions
│
├── core/           ── Core engine
│   ├── agent/        ── Model selector, role classification
│   ├── config/       ── Config loading, path management
│   ├── events/       ── Event bus (Agent, Tool, Session, Context)
│   ├── permissions/  ── Permission engine (build/edit/ask modes)
│   ├── session/      ── Session storage (JSONL), memory management
│   └── tools/        ── Tool interface definitions, registry
│
├── providers/      ── LLM Provider adapters
│   ├── anthropic/    ── Anthropic (Claude)
│   ├── openai/       ── OpenAI (GPT, o-series)
│   ├── gemini/       ── Google Gemini
│   └── openai-compatible/ ── DeepSeek, Groq, SiliconFlow, etc.
│
├── tools/          ── 15 built-in tool implementations
│   ├── bash/         ── Shell command execution
│   ├── file/         ── read/write/edit/tree
│   ├── search/       ── glob/grep
│   ├── web/          ── fetch/search
│   ├── lsp/          ── TypeScript diagnostics/references
│   ├── todo/         ── TODO management
│   ├── tokens/       ── Token estimation
│   └── sessions/     ── Session list/search
│
├── mcp/            ── MCP protocol bridge
├── skills/         ── Skill registry system
├── server/         ── HTTP API server
└── vscode/         ── VSCode extension
```

### Event System

Unified event naming that abstracts provider differences:

| Event | Description |
|-------|-------------|
| `agent:thinking` | Agent starts thinking |
| `agent:reasoning` | Reasoning token stream |
| `agent:reasoning_done` | Reasoning complete |
| `agent:text` | Text token stream |
| `agent:text_done` | Text output complete |
| `agent:tool_call` | Tool invocation |
| `agent:error` | Error |
| `agent:done` | Turn complete |
| `tool:start` | Tool execution start |
| `tool:end` | Tool execution complete |
| `session:start` | Session start |
| `session:end` | Session end |
| `context:update` | Context update |
| `context:truncate` | Context truncation |
| `context:summary` | Context summary |

---

## Supported LLM Providers

| Provider | Type | Example Models | Notes |
|----------|------|----------------|-------|
| **Anthropic** | Native SDK | claude-sonnet-4-6, claude-opus-4-7 | Extended Thinking |
| **OpenAI** | Native SDK | gpt-4o, o3, o4-mini | Reasoning (o-series) |
| **Google Gemini** | Native SDK | gemini-2.0-flash, gemini-pro | |
| **DeepSeek** | OpenAI Compatible | deepseek-chat, deepseek-r1 | Deep Thinking |
| **Groq** | OpenAI Compatible | llama-3.3-70b-versatile | Fast inference |
| **SiliconFlow** | OpenAI Compatible | Qwen/Qwen2.5-72B-Instruct | China region |
| **OpenRouter** | OpenAI Compatible | anthropic/claude-sonnet-4-6 | Aggregated routing |
| **Mistral** | OpenAI Compatible | mistral-large-latest | |
| **Together AI** | OpenAI Compatible | Llama-3-70b-chat-hf | |
| **Moonshot** | OpenAI Compatible | moonshot-v1-8k | China region |
| **Zhipu AI** | OpenAI Compatible | glm-4 | China region |
| **Baidu ERNIE** | OpenAI Compatible | ernie-4.0-turbo-8k | China region |
| **xAI** | OpenAI Compatible | grok-3 | |
| **Cerebras** | OpenAI Compatible | llama-3.3-70b | Fast inference |
| **Perplexity** | OpenAI Compatible | sonar | Search augmented |
| **Ollama** | OpenAI Compatible | llama3.2, qwen2.5 | Local models |
| **LM Studio** | OpenAI Compatible | local-model | Local models |

> Also supports any OpenAI-compatible API — just configure `baseUrl`.

---

## Roadmap

- [x] Multi-provider support (20+)
- [x] 15 built-in tools
- [x] TUI terminal interface (React/ink)
- [x] Extended Thinking / Reasoning
- [x] Orchestrator multi-model delegation
- [x] Session persistence (JSONL)
- [x] MCP protocol integration
- [x] Plugin system
- [ ] Permission system — real ask confirmation flow
- [ ] Session Picker — interactive session selection
- [ ] Multiline input + paste detection
- [ ] File change detection (Snapshot system)
- [ ] Virtual scrolling + tool output folding
- [ ] Global keybinding system (Keymap)
- [ ] Dialog / Modal unified management
- [ ] API Key rotation
- [ ] Automatic context compression
- [ ] VSCode extension

---

## License

This project is licensed under AGPL-3.0 — see [LICENSE](LICENSE) for details.

---

## Acknowledgments

- Reference projects:
    - [OpenCode](https://github.com/nicepkg/opencode) — TUI architecture, Permission system, Dialog design
    - [Claude Code](https://github.com/anthropics/claude-code) — Paste handling, Permission interaction, Tool system
    - [NekoBot](https://github.com/Carillen/NekoBot) — Framework design, Plugin system
- All contributors

---

<div align="center">

**If this project helps you, please give it a Star ⭐**

Made with ❤️ by Carillen & OfficialNekoTeam
</div>
