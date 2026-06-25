# NekoCLI

<div align="center">

**Terminal AI Coding Assistant — Multi-Provider, Multi-Tool, Multi-Model Orchestration**

(Currently in early development. Version stays at 0.x.x until official release. Following [Semantic Versioning 2.0](https://semver.org/). Please report issues via GitHub Issues.)

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](https://github.com/Ringaire/NekoCLI)
[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-orange.svg)](LICENSE)

A powerful, extensible terminal AI coding assistant with 20+ LLM provider support and 15 built-in developer tools.

[Features](#features) • [Quick Start](#quick-start) • [Architecture](#architecture) • [Supported Providers](#supported-llm-providers) • [Roadmap](#roadmap)

</div>

---

## Features

### Core Features

- 🤖 **Multi-Provider Support** — Anthropic, OpenAI, Google Gemini, DeepSeek, Groq, 20+ providers
- 🛠️ **15 Built-in Tools** — bash, file operations, search, web, LSP, TODO, Token counting, session management
- 🖥️ **TUI Interface** — Ratatui-based terminal UI with markdown rendering, streaming output, reasoning display
- 🧠 **Extended Thinking** — Support for Anthropic, OpenAI o-series, DeepSeek reasoning
- 🎭 **Orchestrator Mode** — Multi-model sub-agent delegation with automatic model role selection
- 🔐 **Permission System** — build/edit/ask modes with granular tool permission control
- 📦 **Session Persistence** — JSONL storage with history recovery and compaction
- 🔌 **MCP Protocol** — Model Context Protocol integration
- 🧩 **Plugin System** — Extensible via npm packages

### Tool List

| Tool | Description |
|------|------------|
| `bash` | Execute shell commands |
| `read_file` | Read files (with line range slicing) |
| `edit_file` | Precise file content replacement |
| `write_file` | Write files (auto-create directories) |
| `tree` | Show directory tree (auto-excludes build artifacts) |
| `glob` | Search files by pattern |
| `grep` | Search file contents with ripgrep |
| `web_fetch` | Fetch web page content |
| `web_search` | DuckDuckGo search |
| `lsp_diagnostics` | TypeScript type checking |
| `lsp_refs` | Find symbol references |
| `todo` | Session-level TODO management |
| `token_count` | Token estimation |
| `list_sessions` | List saved sessions |
| `search_sessions` | Search session history |

---

## Quick Start

### Prerequisites

- Rust 1.85+
- Network connection (for LLM API calls)

### Installation

1. Clone the repo
```bash
git clone https://github.com/Ringaire/NekoCLI.git
cd NekoCLI
```

2. Build
```bash
cargo build --release
```

3. Configure Provider
```bash
# Set API Key (choose one or more)
export ANTHROPIC_API_KEY="sk-ant-xxx"
export OPENAI_API_KEY="sk-xxx"
export DEEPSEEK_API_KEY="sk-xxx"
```

4. Install to system (recommended)
```bash
cargo install --path . --bin neko
neko
```

5. Or run directly (no install)
```bash
cargo run --release
```

### Configuration

Config file at `~/.config/neko/settings.jsonc` (XDG spec), supports JSONC format (with comments).

```jsonc
{
  // Default model (provider/model-id format)
  "model": "anthropic/claude-sonnet-4-6",

  // Provider configuration
  "providers": {
    "anthropic": {
      "apiKey": "sk-ant-xxx"  // or use env var ANTHROPIC_API_KEY
    },
    "deepseek": {
      "apiKey": "sk-xxx"
    }
  }
}
```

Provider definitions at `~/.config/neko/providers.json` (global) or `.neko/providers.json` (project-level).

---

## Commands

Type `/` in the TUI to see all commands:

| Command | Description |
|---------|-------------|
| `/help` | Show full command list |
| `/model [id]` | Switch or search models |
| `/model refresh` | Re-fetch model list |
| `/model reload` | Reload config + refresh cache |
| `/connect [provider] [key]` | Configure provider |
| `/sessions [id]` | List or load sessions |
| `/new` | New session |
| `/compact` | Compact history (summarize) |
| `/think [on\|off] [budget]` | Toggle extended thinking |
| `/orchestrate` | Toggle orchestrator mode |
| `/review [args]` | Code review (git diff) |
| `/diff` | Show git diff |
| `/init` | Generate AGENTS.md |
| `/allow <tool>` | Allow tool |
| `/deny <tool>` | Deny tool |
| `/plugin install <pkg>` | Install plugin |
| `/reload` | Hot-reload config, MCP, skills |
| `/exit` | Exit |

### Hotkeys

| Key | Description |
|-----|-------------|
| `Tab` | Switch mode (build → edit → ask) |
| `↑ / ↓` | Browse input history |
| `Ctrl+A / Ctrl+E` | Line start / end |
| `Ctrl+C` | Clear input or exit |
| `@file.ts` | Attach file or directory |

---

## Architecture

```
crates/
├── neko-cli/        ── CLI entry & TUI (Ratatui)
│   ├── src/agent/    ── Agent orchestration, Turn loop, Model discovery
│   ├── src/tui/      ── Ratatui components (App, MessageList, PromptInput)
│   └── src/repl/     ── REPL command handling, Mode switching
│
├── neko-core/       ── Core engine
│   ├── agent/        ── Model selector, Role classifier
│   ├── config/       ── Config loading, Path management
│   ├── events/       ── Event bus (Agent, Tool, Session, Context)
│   ├── permissions/  ── Permission engine (build/edit/ask modes)
│   ├── session/      ── Session storage (JSONL), Memory management
│   └── tools/        ── Tool interface definitions, Registry
│
├── neko-providers/  ── LLM Provider adapters
│   ├── anthropic/    ── Anthropic (Claude)
│   ├── openai/       ── OpenAI (GPT, o-series)
│   ├── gemini/       ── Google Gemini
│   └── compatible/   ── DeepSeek, Groq, SiliconFlow, etc.
│
├── neko-tools/      ── 15 built-in tools
│   ├── bash/         ── Shell command execution
│   ├── file/         ── Read/Write/Edit/Tree
│   ├── search/       ── glob/grep
│   ├── web/          ── fetch/search
│   ├── lsp/          ── TypeScript diagnostics/references
│   ├── todo/         ── TODO management
│   ├── tokens/       ── Token estimation
│   └── sessions/     ── Session list/search
│
├── neko-mcp/        ── MCP protocol bridge
└── neko-skills/     ── Skill registry
```

### Event System

Unified event naming, provider-agnostic:

| Event | Description |
|-------|-------------|
| `agent:thinking` | Agent starts thinking |
| `agent:reasoning` | Reasoning token stream |
| `agent:reasoning_done` | Reasoning complete |
| `agent:text` | Text token stream |
| `agent:text_done` | Text output complete |
| `agent:tool_call` | Tool call |
| `agent:error` | Error |
| `agent:done` | Turn complete |
| `tool:start` | Tool execution starts |
| `tool:end` | Tool execution ends |
| `session:start` | Session starts |
| `session:end` | Session ends |
| `context:update` | Context update |
| `context:truncate` | Context truncation |
| `context:summary` | Context summary |

---

## Supported LLM Providers

| Provider | Type | Example Models | Notes |
|----------|------|---------------|-------|
| **Anthropic** | Native SDK | claude-sonnet-4-6, claude-opus-4-7 | Extended Thinking |
| **OpenAI** | Native SDK | gpt-4o, o3, o4-mini | Reasoning (o-series) |
| **Google Gemini** | Native SDK | gemini-2.0-flash, gemini-pro | |
| **DeepSeek** | OpenAI Compat | deepseek-chat, deepseek-r1 | Deep Think |
| **Groq** | OpenAI Compat | llama-3.3-70b-versatile | Fast inference |
| **SiliconFlow** | OpenAI Compat | Qwen/Qwen2.5-72B-Instruct | China service |
| **OpenRouter** | OpenAI Compat | anthropic/claude-sonnet-4-6 | Aggregation |
| **Mistral** | OpenAI Compat | mistral-large-latest | |
| **Together AI** | OpenAI Compat | Llama-3-70b-chat-hf | |
| **Moonshot** | OpenAI Compat | moonshot-v1-8k | China service |
| **Zhipu AI** | OpenAI Compat | glm-4 | China service |
| **Baidu ERNIE** | OpenAI Compat | ernie-4.0-turbo-8k | China service |
| **xAI** | OpenAI Compat | grok-3 | |
| **Cerebras** | OpenAI Compat | llama-3.3-70b | Fast inference |
| **Perplexity** | OpenAI Compat | sonar | Search enhanced |
| **Ollama** | OpenAI Compat | llama3.2, qwen2.5 | Local models |
| **LM Studio** | OpenAI Compat | local-model | Local models |

> Any OpenAI-compatible API works by configuring `baseUrl`.

---

## Roadmap

- [x] Multi-provider support (20+)
- [x] 15 built-in tools
- [x] TUI interface (Ratatui)
- [x] Extended Thinking / Reasoning
- [x] Orchestrator multi-model orchestration
- [x] Session persistence (JSONL)
- [x] MCP protocol integration
- [x] Plugin system
- [ ] Permission system — real ask confirmation flow
- [ ] Session Picker — interactive session selection
- [ ] Multi-line input + paste detection
- [ ] File change detection (Snapshot system)
- [ ] Virtual scrolling + tool output folding
- [ ] Global hotkey system (Keymap)
- [ ] Dialog / Modal unified management
- [ ] API Key rotation
- [ ] Automatic context compression
- [ ] VSCode extension

---

## License

AGPL-3.0 — See [LICENSE](LICENSE) for details.

---

## Acknowledgments

- Reference projects:
    - [OpenCode](https://github.com/nicepkg/opencode) — TUI architecture, Permission system, Dialog design
    - [Claude Code](https://github.com/anthropics/claude-code) — Paste handling, Permission interaction, Tool system
    - [NekoBot](https://github.com/Ringaire/NekoBot) — Framework design, Plugin system
- All contributors

---

<div align="center">

**If this project helps you, please give it a Star ⭐**

Made with ❤️ by Ringaire玲汐
</div>
