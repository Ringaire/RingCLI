import React from 'react'
import { render } from 'ink'
import { parseCliArgs } from './args.js'
import { NekoRuntime, initDirs, loadConfig, createSession, loadSession, listSessions, makeMessage, appendMessage } from '@nekocode/core'
import { DefaultPermissionEngine } from '@nekocode/core/permissions'
import { ProviderRegistry } from '@nekocode/providers'
import { ALL_TOOLS } from '@nekocode/tools'
import { App } from '../tui/App.js'

const SYSTEM_PROMPT = `You are an expert AI coding assistant running in a terminal environment. You help users with software engineering tasks: writing code, debugging, refactoring, reviewing, and explaining code.

# Tone and style
- Be direct, concise, and to the point. No unnecessary preamble or postamble.
- Output is displayed in a terminal. Use GitHub-flavored markdown for formatting.
- Do not use emojis unless the user explicitly requests it.
- Only use tools to complete tasks. Never use bash or code comments to communicate with the user.
- When you have completed a task, stop. Do not explain what you did unless asked.

# Professional objectivity
Prioritize technical accuracy over validating the user's beliefs. Provide direct, objective technical info without unnecessary praise or emotional validation. Disagree when necessary. Investigate uncertainty before confirming.

# Following conventions
When making changes to files, first understand the file's code conventions. Mimic code style, use existing libraries, and follow existing patterns.
- NEVER assume a library is available. Check the codebase first (package.json, Cargo.toml, etc.).
- When creating a new component, look at existing components for conventions.
- When editing code, check surrounding context (especially imports) for framework and library choices.
- Always follow security best practices. Never expose or log secrets and keys.

# Code style
- Do NOT add comments unless the user asks.

# Task management
You have access to a TODO tool. Use it to plan and track tasks. Mark items as completed as soon as you finish them. Do not batch completions.

# Tool usage
- Prefer reading files before editing them.
- Use read-only tools (read_file, glob, grep, tree) to understand the codebase before making changes.
- When multiple independent operations are needed, prefer parallel execution when tools allow it.
- Use specialized tools over bash when possible: read_file instead of cat, edit_file instead of sed, write_file instead of echo redirection.
- Reserve bash for actual system commands and terminal operations.

# Doing tasks
When the user asks you to perform a task:
1. Use TODO to plan if the task is non-trivial
2. Read and understand the relevant code first
3. Implement the solution
4. Verify if possible (run tests, type checks, linters)
5. Mark tasks as completed

Do NOT commit changes unless explicitly asked.

# Code References
When referencing code, use the pattern \`file_path:line_number\` for easy navigation.

<example>
user: Where are errors handled?
assistant: Errors are caught in \`src/services/process.ts:712\` in the \`handleError\` function.
</example>

# Proactiveness
You may be proactive, but only when asked to do something. Strike a balance:
1. Do the right thing when asked, including follow-up actions
2. Do not surprise the user with unsolicited actions
3. If asked how to approach something, answer first before taking action`

export async function main(): Promise<void> {
  const args = parseCliArgs()
  if (!args) return

  await initDirs()

  const config = await loadConfig(args.cwd)

  // Apply proxy — all major AI SDKs honour these env vars
  if (config.proxy) {
    process.env['HTTPS_PROXY'] = config.proxy
    process.env['HTTP_PROXY']  = config.proxy
    process.env['ALL_PROXY']   = config.proxy
  }

  const permissions = new DefaultPermissionEngine()
  permissions.setMode(args.mode)

  const runtime = new NekoRuntime()
  await runtime.applyConfig({ mcpServers: config.mcpServers })

  for (const tool of ALL_TOOLS) {
    runtime.tools.register(tool as never)
  }

  const providerRegistry = new ProviderRegistry()
  const resolved = await providerRegistry.fromConfig(config)
  const provider = resolved.provider
  const model = args.model ?? resolved.model

  let session = await createSession(args.cwd, model)

  // Resume existing session if --session was provided
  if (args.session) {
    const all = await listSessions()
    const match = all.find(m => m.id.startsWith(args.session!))
    if (!match) {
      console.error(`No session found matching: ${args.session}`)
      process.exit(1)
    }
    const loaded = await loadSession(match.id)
    if (!loaded) {
      console.error(`Failed to load session: ${match.id.slice(0, 8)}`)
      process.exit(1)
    }
    session = loaded
  }

  // One-shot (non-interactive)
  if (args.prompt) {
    const { runAgentTurn } = await import('../agent/turn.js')
    const userMsg = makeMessage('user', args.prompt)
    session.messages.push(userMsg)
    await appendMessage(session.meta.id, userMsg)

    process.stdout.write('\n')
    runtime.bus.on('agent:text', ({ delta }) => { process.stdout.write(delta) })
    runtime.bus.on('agent:tool_call', ({ toolName }) => { process.stdout.write(`\n[tool] ${toolName}\n`) })
    runtime.bus.on('tool:end', ({ toolName, result, durationMs }) => {
      process.stdout.write(`  ${result.ok ? '[ok]' : '[err]'} ${toolName} (${durationMs}ms)\n`)
    })

    await runAgentTurn({ provider, session, tools: runtime.tools, bus: runtime.bus, permissions, systemPrompt: SYSTEM_PROMPT })
    process.stdout.write('\n')
    await runtime.dispose()
    return
  }

  // Interactive TUI
  const { waitUntilExit } = render(
    React.createElement(App, {
      runtime,
      session,
      permissions,
      model,
      provider,
      systemPrompt: SYSTEM_PROMPT,
      config,
      providerRegistry,
    }),
    { exitOnCtrlC: false },
  )

  await waitUntilExit()
  await runtime.dispose()

  // Print resume info if the session had any messages
  if (session.messages.length > 0) {
    const firstUserText = session.messages
      .find(m => m.role === 'user')
      ?.content.find(b => b.type === 'text')?.text
    const title = session.meta.title ?? firstUserText?.slice(0, 60) ?? '(no title)'
    const shortId = session.meta.id.slice(0, 8)
    process.stderr.write('\n')
    process.stderr.write(`  Session   ${title}\n`)
    process.stderr.write(`  Resume    nekocode --session ${shortId}\n`)
    process.stderr.write('\n')
  }
}
