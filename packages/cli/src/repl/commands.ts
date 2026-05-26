/**
 * REPL commands — processed inside the conversation loop before anything
 * reaches the model. These are session-level controls, not shell commands.
 *
 * Invoked by typing /commandname [args] in the input.
 */

import { execSync } from 'node:child_process'
import type { NekoRuntime } from '@nekocode/core'
import type { Session } from '@nekocode/core'
import { createSession, listSessions, loadSession } from '@nekocode/core'
import type { DefaultPermissionEngine } from '@nekocode/core/permissions'
import { MODE_DESCRIPTIONS } from '@nekocode/core/permissions'
import { PRESETS } from '@nekocode/providers'

const DEFAULT_MODELS: Record<string, string> = {
  anthropic:   'claude-sonnet-4-6',
  openai:      'gpt-4o',
  gemini:      'gemini-2.0-flash',
  deepseek:    'deepseek-chat',
  groq:        'llama-3.3-70b-versatile',
  siliconflow: 'Qwen/Qwen2.5-72B-Instruct',
  openrouter:  'anthropic/claude-sonnet-4-6',
  mistral:     'mistral-large-latest',
  together:    'meta-llama/Llama-3-70b-chat-hf',
  moonshot:    'moonshot-v1-8k',
  zhipu:       'glm-4',
  baidu:       'ernie-4.0-turbo-8k',
  xai:         'grok-3',
  cerebras:    'llama-3.3-70b',
  deepinfra:   'meta-llama/Llama-4-Scout-17B-16E-Instruct',
  fireworks:   'accounts/fireworks/models/llama-v3p3-70b-instruct',
  baseten:     'mistral-7b-instruct',
  nvidia:      'meta/llama-3.3-70b-instruct',
  perplexity:  'sonar',
  cohere:      'command-r-plus',
  ollama:      'llama3.2',
  lmstudio:    'local-model',
}
import { renderStatus } from '../commands/status.js'
import { pluginInstall, pluginList, pluginRemove } from '../commands/plugin.js'
import { runRc, formatRcOutput } from './rc.js'

// ── Model fetching ───────────────────────────────────────────────────────────

/**
 * Fetch available model IDs from a provider's /models endpoint.
 * Tries OpenAI-compatible format first, then raw array.
 */
async function fetchProviderModels(
  baseUrl: string | undefined,
  apiKey: string | undefined,
): Promise<string[] | null> {
  if (!baseUrl) return null
  try {
    const url = baseUrl.replace(/\/+$/, '') + '/models'
    const headers: Record<string, string> = {}
    if (apiKey) headers['Authorization'] = `Bearer ${apiKey}`
    const res = await fetch(url, { headers, signal: AbortSignal.timeout(8000) })
    if (!res.ok) return null
    const data = await res.json() as
      | { data: Array<{ id: string }> }
      | Array<{ id: string }>
    const items = Array.isArray(data) ? data : (data.data ?? [])
    const ids = items
      .map(m => m.id)
      .filter((id): id is string => typeof id === 'string' && id.length > 0)
    return ids.length > 0 ? ids : null
  } catch {
    return null
  }
}

export interface ReplCommandContext {
  runtime: NekoRuntime
  session: Session
  model: string
  setModel: (model: string) => void
  permissions: DefaultPermissionEngine
  print: (text: string) => void
  clearSession: () => void
  replaceSession: (session: Session) => void
  exit: () => void
}

export interface ReplCommandResult {
  /** true if the command was handled (don't pass input to model) */
  handled: boolean
  /** Text to display in the REPL */
  output?: string
  /** If set, inject this as the full prompt for the next agent turn */
  injectPrompt?: string
  /** If set, open an interactive modal in the TUI */
  openModal?: 'provider-setup'
  /** If set, toggle orchestrator mode on/off */
  toggleOrchestrator?: boolean
  /** If set, update thinking mode */
  setThinking?: { enabled: boolean; budget: number }
  /** If true, trigger session compact in App */
  compact?: boolean
  /** If set, reload provider from config and switch to this model */
  reloadProvider?: string
}

// ── Prompt templates (adapted from OpenCode) ─────────────────────────────────

const PROMPT_REVIEW = `\
You are a code reviewer. Review code changes and provide actionable feedback.

$ARGUMENTS

## What to review

If no arguments: review all uncommitted changes (run \`git diff\` and \`git diff --cached\` and \`git status --short\`).
If a commit hash: review that commit (\`git show $ARGUMENTS\`).
If a branch name: compare to current branch (\`git diff $ARGUMENTS...HEAD\`).

After getting the diff, read the full files changed to understand context.

## What to look for

- **Bugs**: logic errors, missing guards, edge cases, security issues, broken error handling
- **Structure**: follows existing patterns, uses established abstractions, not overly nested
- **Performance**: only flag obvious issues (O(n²) on unbounded data, blocking I/O on hot paths)
- **Behavior changes**: flag unintentional ones

## Output rules

- Be direct and clear. No flattery.
- State severity. Give the scenario that triggers each issue.
- Only review the changed code, not pre-existing code.`

const PROMPT_INIT = `\
Create or update \`AGENTS.md\` for this repository.

Goal: a compact instruction file that helps future NekoCode sessions avoid mistakes and ramp up quickly.
Every line should answer: "Would an agent likely miss this without help?" If not, leave it out.

$ARGUMENTS

## How to investigate

Read the highest-value sources first:
- README, root manifests, workspace config, lockfiles
- build, test, lint, formatter, typecheck, codegen config
- CI workflows and pre-commit / task runner config
- existing instruction files (AGENTS.md, CLAUDE.md, .cursor/rules/, etc.)

If architecture is still unclear, inspect a few representative code files to find real entrypoints,
package boundaries, and execution flow.

## What to extract

- exact developer commands, especially non-obvious ones
- how to run a single test, package, or verification step
- required command order when it matters
- monorepo/multi-package boundaries and real entrypoints
- framework quirks: generated code, migrations, codegen, build artifacts
- repo-specific style or workflow conventions that differ from defaults
- testing quirks: fixtures, integration prerequisites, flaky tests

## Writing rules

Include only high-signal, repo-specific guidance. Exclude generic software advice.
Prefer short sections and bullets. If AGENTS.md already exists, improve it in place.`

// ── Help text ─────────────────────────────────────────────────────────────────

const HELP = `
┌─ NekoCode Commands ─────────────────────────────────────────┐
│ Mode  (Tab on empty input to cycle)                          │
│   build → edit → ask → build ...                             │
│                                                              │
│ Session                                                      │
│   /new                   New session (clear history)        │
│   /sessions [id]         List sessions, or load one by id   │
│   /compact               Summarize history to save tokens   │
│   /rename <name>         Rename current session             │
│   /clear                 Clear conversation history         │
│   /status                Token usage, tools, skills         │
│   /skills                List loaded skills                 │
│   /exit                  Exit NekoCode                      │
│                                                              │
│ Agent                                                        │
│   /model [id]            Show cached models, fuzzy search, or switch  │
│   /model refresh         Re-fetch model list from provider API        │
│   /model reload          Reload model config + refresh cache          │
│   /connect [prov] [key]  Configure provider connection      │
│   /mcp <name> <cmd>      Add MCP server for this session    │
│   /think [on|off] [N]    Extended thinking (N=token budget) │
│   /orchestrate           Toggle orchestrator (multi-agent)  │
│                                                              │
│ Code                                                         │
│   /review [args]         Code review (git diff by default)  │
│   /diff                  Show git diff in chat              │
│   /init [focus]          Generate/update AGENTS.md          │
│                                                              │
│ Permissions                                                  │
│   /allow <tool> [path]   Always allow a tool               │
│   /deny  <tool> [path]   Always deny a tool                │
│   /perms                 Show active permission rules       │
│                                                              │
│ Plugins                                                      │
│   /plugin install <pkg>  Install from npm                   │
│   /plugin list           List installed                     │
│   /plugin remove  <pkg>  Uninstall                          │
│                                                              │
│ Config                                                       │
│   /reload                Hot-reload config, MCP, skills     │
│                                                              │
│ @ Mentions (in message)                                      │
│   @path/file.ts   Attach file content                       │
│   @src/           Attach directory tree                     │
└──────────────────────────────────────────────────────────────┘
`.trim()

// ── Handler ────────────────────────────────────────────────────────────────────

export async function handleReplCommand(
  name: string,
  args: string,
  ctx: ReplCommandContext,
): Promise<ReplCommandResult> {

  switch (name) {

    // ── Session ───────────────────────────────────────────────────────────────
    case 'compact': {
      if (ctx.session.messages.length === 0) {
        return { handled: true, output: 'Nothing to compact — conversation is empty.' }
      }
      return { handled: true, compact: true }
    }

    case 'sessions': {
      const prefix = args.trim()
      const all = await listSessions()

      // Load session by ID prefix
      if (prefix) {
        const match = all.find(m => m.id.startsWith(prefix))
        if (!match) return { handled: true, output: `No session matching: ${prefix}` }
        const sess = await loadSession(match.id)
        if (!sess) return { handled: true, output: `Failed to load session: ${match.id.slice(0, 8)}` }
        ctx.replaceSession(sess)
        return {
          handled: true,
          output: `Session loaded: ${match.id.slice(0, 8)}  ${match.title ?? '(no title)'}  (${sess.messages.length} messages)`,
        }
      }

      // List sessions
      if (all.length === 0) return { handled: true, output: 'No saved sessions.' }

      const now = Date.now()
      const lines = ['Saved sessions (newest first):', '']
      for (let i = 0; i < all.length; i++) {
        const m = all[i]!
        const age = now - m.updatedAt
        const timeAgo = age < 60_000 ? 'just now'
          : age < 3_600_000 ? `${Math.floor(age / 60_000)}m ago`
          : age < 86_400_000 ? `${Math.floor(age / 3_600_000)}h ago`
          : `${Math.floor(age / 86_400_000)}d ago`
        const title = (m.title ?? '(no title)').slice(0, 40).padEnd(40)
        const msgs  = String(m.messageCount).padStart(3)
        lines.push(`  ${m.id.slice(0, 8)}  ${title}  ${msgs} msgs  ${timeAgo}`)
      }
      lines.push('')
      lines.push('Use /sessions <id> to load one.')
      return { handled: true, output: lines.join('\n') }
    }

    case 'new': {
      const session = await createSession(ctx.session.meta.cwd, ctx.model)
      ctx.replaceSession(session)
      ctx.clearSession()
      return { handled: true, output: `New session started: ${session.meta.id.slice(0, 8)}` }
    }

    case 'rename': {
      const title = args.trim()
      if (!title) return { handled: true, output: 'Usage: /rename <name>' }
      ctx.session.meta.title = title
      return { handled: true, output: `Session renamed to: ${title}` }
    }

    case 'status': {
      return { handled: true, output: renderStatus(ctx.runtime, ctx.session, ctx.model) }
    }

    case 'skills': {
      const listing = ctx.runtime.skills?.buildListing()
      return { handled: true, output: listing || 'No skills loaded.' }
    }

    case 'clear': {
      ctx.clearSession()
      return { handled: true, output: 'Conversation cleared.' }
    }

    case 'exit':
    case 'quit':
    case 'q': {
      ctx.exit()
      return { handled: true }
    }

    case 'help': {
      return { handled: true, output: HELP }
    }

    // ── Agent / Model ─────────────────────────────────────────────────────────
    case 'model': {
      const target = args.trim()

      // /model reload → reload model config (re-read settings, re-instantiate provider, refresh cache)
      if (target === 'reload') {
        const { loadConfig } = await import('@nekocode/core')
        const cfg = await loadConfig()
        const currentProvider = (cfg.model ?? '').split('/')[0] ?? 'anthropic'
        const preset = PRESETS[currentProvider]
        const entry = cfg.providers?.[currentProvider]
        const resolvedApiKey = entry?.apiKey ?? (preset?.apiKeyEnv ? process.env[preset.apiKeyEnv] : undefined)
        const resolvedBaseUrl = entry?.baseUrl ?? preset?.baseUrl

        const lines = ['Model config reloaded.']

        // Re-instantiate provider
        ctx.setModel(cfg.model ?? 'anthropic/claude-sonnet-4-6')
        lines.push(`  Model: ${cfg.model}`)

        // Refresh model cache
        try {
          const models = await fetchProviderModels(resolvedBaseUrl, resolvedApiKey)
          if (models && models.length > 0) {
            cfg.models ??= {}
            cfg.models[currentProvider] = models
            const { saveConfig } = await import('@nekocode/core')
            await saveConfig(cfg)
            lines.push(`  Cached: ${models.length} models for ${currentProvider}`)
          }
        } catch {
          lines.push(`  Model cache: fetch failed (using existing cache)`)
        }

        return { handled: true, output: lines.join('\n') }
      }

      // /model refresh → re-fetch model list only (no provider reload)
      if (target === 'refresh') {
        const { loadConfig, saveConfig } = await import('@nekocode/core')
        const cfg = await loadConfig()
        const currentProvider = (cfg.model ?? '').split('/')[0] ?? 'anthropic'
        const preset = PRESETS[currentProvider]
        const entry = cfg.providers?.[currentProvider]
        const resolvedApiKey = entry?.apiKey ?? (preset?.apiKeyEnv ? process.env[preset.apiKeyEnv] : undefined)
        const resolvedBaseUrl = entry?.baseUrl ?? preset?.baseUrl

        try {
          const models = await fetchProviderModels(resolvedBaseUrl, resolvedApiKey)
          if (models && models.length > 0) {
            cfg.models ??= {}
            cfg.models[currentProvider] = models
            await saveConfig(cfg)
            return { handled: true, output: `Refreshed ${models.length} models for ${currentProvider}` }
          }
          return { handled: true, output: `No models returned from ${currentProvider} API` }
        } catch {
          return { handled: true, output: `Failed to fetch models from ${currentProvider}` }
        }
      }

      // /model <provider/model-id> → switch model directly
      if (target && target.includes('/')) {
        ctx.setModel(target)
        return { handled: true, output: `Model switched to: ${target}` }
      }

      // /model <name> → fuzzy search across cached models
      if (target) {
        const { loadConfig } = await import('@nekocode/core')
        const cfg = await loadConfig()
        const currentProvider = (cfg.model ?? '').split('/')[0] ?? 'anthropic'
        const cached = cfg.models?.[currentProvider] ?? []

        if (cached.length > 0) {
          const q = target.toLowerCase()
          const matches = cached.filter(m => m.toLowerCase().includes(q))
          if (matches.length === 1) {
            const full = `${currentProvider}/${matches[0]}`
            ctx.setModel(full)
            return { handled: true, output: `Model switched to: ${full}` }
          }
          if (matches.length > 1) {
            const lines = [`Matches for "${target}" in ${currentProvider}:`, '']
            for (const m of matches.slice(0, 20)) {
              lines.push(`  ${currentProvider}/${m}`)
            }
            if (matches.length > 20) lines.push(`  ... and ${matches.length - 20} more`)
            lines.push('', 'Use /model provider/model-id to switch')
            return { handled: true, output: lines.join('\n') }
          }
        }

        // Fallback: treat as direct model id
        ctx.setModel(target.includes('/') ? target : `${currentProvider}/${target}`)
        return { handled: true, output: `Model switched to: ${target}` }
      }

      // /model (no args) → show current model + cached models for current provider
      const { loadConfig } = await import('@nekocode/core')
      const cfg = await loadConfig()
      const currentProvider = (cfg.model ?? '').split('/')[0] ?? 'anthropic'
      const cached = cfg.models?.[currentProvider] ?? []

      const lines = [`Current model: ${ctx.model}`, '']

      if (cached.length > 0) {
        lines.push(`Cached models for ${currentProvider} (${cached.length}):`)
        for (const m of cached.slice(0, 30)) {
          const marker = ctx.model === `${currentProvider}/${m}` ? ' ◀' : ''
          lines.push(`  ${currentProvider}/${m}${marker}`)
        }
        if (cached.length > 30) lines.push(`  ... and ${cached.length - 30} more`)
        lines.push('', 'Use /model <name> to search, /model refresh to update list')
      } else {
        lines.push('No cached models. Configure a provider first:')
        lines.push('  /connect <provider> <apiKey>')
        lines.push('', 'Or switch directly:')
        lines.push('  /model anthropic/claude-opus-4-7')
        lines.push('  /model deepseek/deepseek-chat')
      }

      lines.push('', 'Configured providers:')
      for (const [key, p] of Object.entries(PRESETS)) {
        const hasKey = cfg.providers?.[key]?.apiKey || (p.apiKeyEnv && process.env[p.apiKeyEnv])
        const marker = hasKey ? '✓' : ' '
        const env = p.apiKeyEnv ? `  (${p.apiKeyEnv})` : ''
        lines.push(`  [${marker}] ${key.padEnd(14)} ${p.name}${env}`)
      }

      return { handled: true, output: lines.join('\n') }
    }

    case 'connect': {
      // /connect                           → interactive setup TUI
      // /connect <provider> <apikey>       → quick configure
      // /connect <provider> <apikey> <url> → with custom baseUrl
      const parts = args.trim().split(/\s+/).filter(Boolean)
      const providerName = parts[0]?.toLowerCase()

      if (!providerName) {
        return { handled: true, openModal: 'provider-setup' }
      }

      const preset = PRESETS[providerName]
      if (!preset) {
        return { handled: true, output: `Unknown provider: "${providerName}"\nRun /connect to open setup.` }
      }

      const apiKey  = parts[1]
      const baseUrl = parts[2]

      if (!apiKey && preset.apiKeyEnv) {
        return { handled: true, output: `Usage: /connect ${providerName} <apiKey>\nOr run /connect to open the interactive setup.` }
      }

      const { loadConfig, saveConfig } = await import('@nekocode/core')
      const cfg = await loadConfig()
      cfg.providers ??= {}
      cfg.providers[providerName] = {
        ...(apiKey ? { apiKey } : {}),
        ...(baseUrl !== undefined ? { baseUrl } : {}),
      }
      if (cfg.model?.split('/')[0] !== providerName) {
        cfg.model = `${providerName}/${DEFAULT_MODELS[providerName] ?? 'default'}`
      }

      // Fetch and cache model list from provider API
      let modelCount = 0
      try {
        const resolvedApiKey = apiKey ?? (preset.apiKeyEnv ? process.env[preset.apiKeyEnv] : undefined)
        const resolvedBaseUrl = baseUrl ?? preset.baseUrl
        const models = await fetchProviderModels(resolvedBaseUrl, resolvedApiKey)
        if (models && models.length > 0) {
          cfg.models ??= {}
          cfg.models[providerName] = models
          modelCount = models.length
        }
      } catch {
        // Model fetch failed — not critical, continue without caching
      }

      await saveConfig(cfg)

      const modelMsg = modelCount > 0
        ? `\n  Cached ${modelCount} models — use /model to browse`
        : ''

      return {
        handled: true,
        output: `${preset.name} configured — switching to ${cfg.model ?? providerName}${modelMsg}`,
        reloadProvider: cfg.model ?? `${providerName}/${DEFAULT_MODELS[providerName] ?? 'default'}`,
      }
    }

    case 'mcp': {
      // /mcp <name> <command> [args...]  → add MCP server for this session
      const parts = args.trim().split(/\s+/)
      const serverName = parts[0]
      if (!serverName || !parts[1]) {
        return { handled: true, output: 'Usage: /mcp <name> <command> [args...]' }
      }
      try {
        await ctx.runtime.applyConfig({
          mcpServers: {
            [serverName]: { type: 'stdio', command: parts[1]!, args: parts.slice(2) },
          },
        })
        return { handled: true, output: `MCP server connected: ${serverName}` }
      } catch (err) {
        return { handled: true, output: `Failed to connect MCP: ${String(err)}` }
      }
    }

    // ── Code ──────────────────────────────────────────────────────────────────
    case 'review': {
      const prompt = PROMPT_REVIEW.replace(/\$ARGUMENTS/g, args.trim() || '(review all uncommitted changes)')
      return { handled: true, injectPrompt: prompt }
    }

    case 'diff': {
      try {
        const diff = execSync('git diff HEAD', {
          cwd: ctx.session.meta.cwd,
          encoding: 'utf-8',
          timeout: 10_000,
        }).trim()
        if (!diff) return { handled: true, output: 'No changes (git diff HEAD is empty).' }
        return { handled: true, output: `\`\`\`diff\n${diff.slice(0, 8000)}\n\`\`\`` }
      } catch {
        return { handled: true, output: 'git diff failed — not a git repo or git not available.' }
      }
    }

    case 'init': {
      const prompt = PROMPT_INIT.replace(/\$ARGUMENTS/g, args.trim() || '')
      return { handled: true, injectPrompt: prompt }
    }

    // ── Permissions ───────────────────────────────────────────────────────────
    case 'allow': {
      const [tool, path] = args.trim().split(/\s+/)
      if (!tool) return { handled: true, output: 'Usage: /allow <tool> [path]' }
      ctx.permissions.allow(tool, path)
      return { handled: true, output: `Allowed: ${tool}${path ? ` on ${path}` : ''}` }
    }

    case 'deny': {
      const [tool, path] = args.trim().split(/\s+/)
      if (!tool) return { handled: true, output: 'Usage: /deny <tool> [path]' }
      ctx.permissions.deny(tool, path)
      return { handled: true, output: `Denied: ${tool}${path ? ` on ${path}` : ''}` }
    }

    case 'perms':
    case 'permissions': {
      const lines: string[] = [
        `Mode: ${ctx.permissions.mode}  (${MODE_DESCRIPTIONS[ctx.permissions.mode]})`,
        '',
        'Custom rules:',
      ]
      const custom = ctx.permissions.customRules()
      if (custom.length === 0) {
        lines.push('  (none)')
      } else {
        for (const r of custom) {
          lines.push(`  ${r.action.padEnd(5)} ${r.tool}${r.path ? ` @ ${r.path}` : ''}`)
        }
      }
      return { handled: true, output: lines.join('\n') }
    }

    // ── Plugins ───────────────────────────────────────────────────────────────
    case 'plugin': {
      const [sub, ...rest] = args.trim().split(/\s+/)
      const pkg = rest.join(' ').trim()
      if (sub === 'install' && pkg) return { handled: true, output: await pluginInstall(pkg) }
      if (sub === 'remove'  && pkg) return { handled: true, output: await pluginRemove(pkg) }
      if (!sub || sub === 'list' || sub === 'ls') return { handled: true, output: await pluginList() }
      return { handled: true, output: 'Usage: /plugin install|list|remove <pkg>' }
    }

    // ── Project config ────────────────────────────────────────────────────────
    case 'project': {
      const sub = args.trim().split(/\s+/)[0]
      if (!sub || sub === 'init') {
        const { initProjectConfig } = await import('@nekocode/core')
        const cfgPath = await initProjectConfig(ctx.session.meta.cwd)
        return { handled: true, output: `Project config ready: ${cfgPath}\n  Add .nekocode/settings.local.jsonc to .gitignore for local overrides.` }
      }
      return { handled: true, output: 'Usage: /project init' }
    }

    // ── Thinking / Reasoning ──────────────────────────────────────────────────
    case 'think': {
      // /think           → toggle
      // /think on [N]    → enable with optional token budget
      // /think off       → disable
      const parts = args.trim().split(/\s+/).filter(Boolean)
      const sub = parts[0]?.toLowerCase()
      if (sub === 'off') {
        return { handled: true, setThinking: { enabled: false, budget: 8000 } }
      }
      const budget = parseInt(parts[sub === 'on' ? 1 : 0] ?? '', 10)
      const validBudget = Number.isFinite(budget) && budget > 0 ? budget : 8000
      return { handled: true, setThinking: { enabled: true, budget: validBudget } }
    }

    // ── Orchestrator ──────────────────────────────────────────────────────────
    case 'orchestrate':
    case 'orch': {
      return { handled: true, toggleOrchestrator: true }
    }

    // ── Reload ────────────────────────────────────────────────────────────────
    case 'reload': {
      const { reloadAll } = await import('../commands/reload.js')
      const result = await reloadAll(ctx.runtime)
      const lines = ['Reload complete.']
      if (result.mcpReloaded.length) lines.push(`  MCP: ${result.mcpReloaded.join(', ')}`)
      if (result.skillsLoaded.length) lines.push(`  Skills: ${result.skillsLoaded.join(', ')}`)
      if (result.errors.length) lines.push(`  Errors:\n${result.errors.map(e => '    ' + e).join('\n')}`)
      return { handled: true, output: lines.join('\n') }
    }

    // ── Hidden ────────────────────────────────────────────────────────────────
    case 'rc': {
      if (!args.trim()) return { handled: true, output: 'Usage: /rc <shell command>' }
      const result = await runRc(args.trim(), ctx.session.meta.cwd)
      return { handled: true, output: formatRcOutput(result, args.trim()) }
    }

    // ── Skill / unknown ───────────────────────────────────────────────────────
    default: {
      const skill = ctx.runtime.skills?.get(name)
      if (skill) {
        return { handled: true, injectPrompt: skill.prompt }
      }
      return { handled: false }
    }
  }
}
