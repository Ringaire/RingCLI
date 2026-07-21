# Contributing to RingCLI

English | [简体中文](CONTRIBUTING.md)

Thanks for your interest in RingCLI! This document explains how to get involved.

---

## Development Setup

- Rust 1.85+
- Git
- An LLM API key (for testing)

```bash
git clone https://github.com/Ringaire/RingCLI.git
cd RingCLI
cargo build
```

---

## Branch Naming

Branch from `develop` using prefix/description format:

| Prefix | Purpose | Example |
|--------|---------|---------|
| `feat/` | New feature or capability | `feat/user-profile-settings` |
| `fix/` | Bug fix | `fix/session-timeout` |
| `docs/` | Documentation or comments only | `docs/api-usage-guide` |
| `refactor/` | Code restructuring, no behavior change | `refactor/payment-service` |
| `perf/` | Performance improvement | `perf/image-lazy-load` |
| `style/` | Formatting, typos, no logic change | `style/fix-typo` |
| `chore/` | Build, toolchain, dependency updates | `chore/update-dependencies` |
| `ci/` | CI/CD config or scripts | `ci/fix-build-pipeline` |

```bash
git checkout develop
git pull origin develop
git checkout -b feat/your-feature
```

---

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]
```

**Types**:

| Type | Purpose |
|------|---------|
| `feat` | New feature |
| `fix` | Bug fix |
| `refactor` | Code restructuring, no behavior change |
| `docs` | Documentation |
| `test` | Tests |
| `chore` | Build, CI, dependencies, tooling |
| `perf` | Performance optimization |
| `style` | Formatting, typos |

**Examples**:

```
feat(tui): add mode picker with scroll selection
fix(provider): max_tokens sent as context window size
docs(readme): remove architecture section, simplify
refactor(engine): move executor from ring-cli to ring-engine
```

- **scope** is optional but encouraged for multi-module projects (e.g. `tui`, `provider`, `engine`, `mcp`)
- **description**: imperative mood, lowercase, no period
- **body**: explain *why*, not *what*

---

## Code Standards

### Rust

- Run `cargo fmt`
- Run `cargo clippy` — no warnings
- Prefer `Option`/`Result` over `unwrap()` (except in tests)
- Document public APIs with `///`
- Keep modules under 500 lines — split if larger

### Tests

```bash
cargo test
```

- Write unit tests for core logic
- Test names should describe behavior: `test_parse_invalid_date_returns_default`
- No need to test trivial getters/setters

### Adding a Provider

1. Create `crates/ring-providers/src/providers/added/{name}/mod.rs`
2. Define `catalog_entry()` function
3. Declare `pub mod {name};` in `added/mod.rs`
4. Register in `catalog.rs` `defaults()`

### Adding a Tool

1. Create `crates/ring-tools/src/tools/{name}.rs`
2. Implement the `Tool` trait
3. Register in `register.rs`

---

## Pull Request Flow

1. Branch from `develop`
2. Develop + ensure tests pass (`cargo test` + `cargo clippy`)
3. Include screenshots if UI changes are involved
4. Open PR against `develop`
5. Wait for review

### PR Title

Same as Conventional Commits:

```
feat(tui): add effort command for reasoning control
```

---

## Project Structure

```
crates/
├── ring-core/       Types, events, permissions, sessions, config
├── ring-providers/  LLM adapters (Anthropic/OpenAI/Gemini/Compatible + OAuth2)
├── ring-tools/      17 built-in tools
├── ring-mcp/        MCP protocol client
├── ring-skills/     Skill loader (SKILL.md)
├── ring-engine/     Agent execution loop, orchestration
└── ring-cli/        TUI, REPL, command system
```

---

## Code of Conduct

- Be respectful and kind
- Communicate in Chinese or English
- When in doubt, open an Issue to discuss first

---

<div align="center">

Questions → Issues · Discussion → Discussions

</div>
