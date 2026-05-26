import React, { useState, useCallback, useEffect, useRef, useMemo } from 'react'
import { Box, useApp, useStdout } from 'ink'
import { randomUUID } from 'node:crypto'
import type { NekoRuntime, Session } from '@nekocode/core'
import { appendMessage, makeMessage, replaceMessages } from '@nekocode/core'
import type { ContentBlock } from '@nekocode/core'
import type { DefaultPermissionEngine, ModeName } from '@nekocode/core/permissions'
import type { ResolvedConfig } from '@nekocode/core/config/schema'
import { StatusBar } from './StatusBar.js'
import { MessageList, type DisplayMessage } from './MessageList.js'
import { PromptInput } from './PromptInput.js'
import { WelcomeBanner } from './WelcomeBanner.js'
import { PermissionPrompt } from './PermissionPrompt.js'
import { ProviderSetup } from './ProviderSetup.js'
import { ModelPicker } from './ModelPicker.js'
import { SessionPicker } from './SessionPicker.js'
import { parseInput } from '../input/parser.js'
import { expandMentions, buildMessageWithMentions } from '../input/mentions.js'
import { handleReplCommand } from '../repl/commands.js'
import { nextMode } from '../repl/mode-tab.js'
import { runAgentTurn } from '../agent/turn.js'
import { runOrchestratorTurn } from '../agent/orchestrator.js'
import { discoverModels } from '../agent/model-discovery.js'
import type { ModelCatalogEntry } from '@nekocode/core/agent/types'
import type { Provider } from '@nekocode/providers/types'
import { ProviderRegistry } from '@nekocode/providers'

const CONTEXT_WINDOW = 200_000

const COMPACT_PROMPT = `Summarize the following conversation concisely but thoroughly.
Preserve all key context: goals, decisions, files discussed, code changes, and the current state of work.
This summary replaces the full history so future turns can continue seamlessly.
Output only the summary, no preamble.`

function sessionToDisplayMessages(messages: import('@nekocode/core').Session['messages']): DisplayMessage[] {
  const display: DisplayMessage[] = []
  const toolMsgIdx = new Map<string, number>()

  for (const m of messages) {
    if (m.role === 'user') {
      const text = m.content.filter(b => b.type === 'text').map(b => b.text ?? '').join('\n')
      if (text) display.push({ id: m.id, role: 'user', text })
    } else if (m.role === 'assistant') {
      const text = m.content.filter(b => b.type === 'text').map(b => b.text ?? '').join('\n')
      if (text) display.push({ id: m.id, role: 'assistant', text })
      for (const b of m.content.filter(b => b.type === 'tool_use')) {
        const idx = display.length
        display.push({ id: b.toolUseId ?? m.id, role: 'tool', text: b.toolName ?? '', toolName: b.toolName ?? '', toolInput: b.toolInput })
        if (b.toolUseId) toolMsgIdx.set(b.toolUseId, idx)
      }
    } else if (m.role === 'tool_result') {
      for (const b of m.content) {
        if (b.type === 'tool_result' && b.toolUseId) {
          const idx = toolMsgIdx.get(b.toolUseId)
          if (idx !== undefined && display[idx] !== undefined) {
            display[idx] = { ...display[idx]!, ok: !b.isError }
          }
        }
      }
    }
  }
  return display
}

function blockToText(b: ContentBlock): string {
  if (b.type === 'text') return b.text ?? ''
  if (b.type === 'tool_use') return `[tool: ${b.toolName}(${JSON.stringify(b.toolInput ?? {}).slice(0, 200)})]`
  if (b.type === 'tool_result') return `[result: ${String(b.toolResult ?? '').slice(0, 400)}]`
  return ''
}

interface Props {
  runtime: NekoRuntime
  session: Session
  permissions: DefaultPermissionEngine
  model: string
  provider: Provider
  systemPrompt: string
  config: ResolvedConfig
  providerRegistry: ProviderRegistry
}

export function App({ runtime, session, permissions, model, provider, systemPrompt, config, providerRegistry }: Props) {
  const { exit } = useApp()
  const { stdout } = useStdout()

  const [messages, setMessages]           = useState<DisplayMessage[]>([])
  const [streamingText, setStreaming]     = useState('')
  const [reasoningText, setReasoning]    = useState('')
  const [inputValue, setInput]            = useState('')
  const [pendingPerm, setPendingPerm]     = useState<{
    callId: string; toolName: string; input: unknown
    resolve: (allowed: boolean) => void
  } | null>(null)
  const [mode, setMode]                   = useState<ModeName>(permissions.mode)
  const [busy, setBusy]                   = useState(false)
  const [currentModel, setCurrentModel]   = useState(model)
  const [showSetup, setShowSetup]           = useState(false)
  const [showModelPicker, setShowModelPicker] = useState(false)
  const [showSessionPicker, setShowSessionPicker] = useState(false)
  const [orchestratorMode, setOrchestrator] = useState(false)
  const [modelCatalog, setModelCatalog]   = useState<ModelCatalogEntry[]>([])
  const [thinkingEnabled, setThinking]   = useState(false)
  const [thinkingBudget, setThinkingBudget] = useState(8000)
  const sessionRef  = useRef(session)
  const providerRef = useRef<Provider>(provider)
  const configRef   = useRef(config)
  const abortRef    = useRef<AbortController | null>(null)
  const permResolverRef = useRef<((allowed: boolean) => void) | null>(null)

  // Input history (persisted only in session)
  const historyRef = useRef<string[]>([])
  const historyIdxRef = useRef(-1)   // -1 = not navigating
  const draftRef = useRef('')        // saved draft while navigating

  // Token estimation from display messages (updates with React state, avoids JSON.stringify on raw tool results)
  const tokens = useMemo(() =>
    messages.reduce((sum, m) => sum + Math.ceil(m.text.length / 3.5), 0),
    [messages]
  )

  // Subscribe to event bus
  useEffect(() => {
    const unsubs = [
      runtime.bus.on('agent:reasoning', ({ delta }) => {
        setReasoning(prev => prev + delta)
      }),
      runtime.bus.on('agent:reasoning_done', ({ full }) => {
        setReasoning('')
        setMessages(prev => [...prev, {
          id: randomUUID(), role: 'reasoning', text: full,
        }])
      }),
      runtime.bus.on('agent:text', ({ delta }) => {
        setStreaming(prev => prev + delta)
      }),
      runtime.bus.on('agent:text_done', ({ full }) => {
        setStreaming('')
        setMessages(prev => [...prev, {
          id: randomUUID(), role: 'assistant', text: full,
        }])
      }),
      runtime.bus.on('agent:tool_call', ({ callId, toolName, input }) => {
        setMessages(prev => [...prev, {
          id: callId, role: 'tool', text: toolName, toolName, toolInput: input,
        }])
      }),
      runtime.bus.on('tool:end', ({ callId, result, durationMs }) => {
        setMessages(prev => prev.map(m =>
          m.id === callId ? { ...m, ok: result.ok, durationMs } : m,
        ))
      }),
      runtime.bus.on('agent:error', ({ error }) => {
        setMessages(prev => [...prev, { id: randomUUID(), role: 'error', text: error }])
      }),
      runtime.bus.on('agent:done', () => {
        setBusy(false)
      }),
    ]
    return () => { for (const u of unsubs) u() }
  }, [runtime.bus])

  // ── Model switching (shared between command ctx and ModelPicker) ──────────────
  const switchModel = useCallback((m: string) => {
    const slash = m.indexOf('/')
    const modelId = slash === -1 ? m : m.slice(slash + 1)
    sessionRef.current.meta.model = modelId
    setCurrentModel(m)
    const newCfg = { ...configRef.current, model: m }
    providerRegistry.fromConfig(newCfg).then(resolved => {
      providerRef.current = resolved.provider
    }).catch(() => { /* keep existing provider */ })
  }, [providerRegistry])

  // ── Session loading (shared between command ctx and SessionPicker) ────────────
  const loadAndReplaceSession = useCallback((sess: typeof session) => {
    sessionRef.current = sess
    setMessages(sessionToDisplayMessages(sess.messages))
    setStreaming('')
    setReasoning('')
    setBusy(false)
    abortRef.current?.abort()
    abortRef.current = null
    // Sync model display from loaded session (#17)
    // meta.model is just the model ID; preserve current provider prefix if present
    if (sess.meta.model) {
      setCurrentModel(prev => {
        const slash = prev.indexOf('/')
        const prov  = slash === -1 ? '' : prev.slice(0, slash + 1)
        return prov + sess.meta.model!
      })
    }
  }, [])

  const onCtrlC = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort()
      abortRef.current = null
    } else {
      void runtime.dispose().then(() => exit())
    }
  }, [runtime, exit])

  const requestPermission = useCallback((callId: string, toolName: string, input: unknown): Promise<boolean> => {
    return new Promise<boolean>(resolve => {
      permResolverRef.current = resolve
      setPendingPerm({ callId, toolName, input, resolve })
    })
  }, [])

  const runTurn = useCallback(async (userText: string, injectPrompt?: string) => {
    setBusy(true)
    const ac = new AbortController()
    abortRef.current = ac
    const sess = sessionRef.current

    // Persist and show user message
    if (userText) {
      const userMsg = makeMessage('user', userText)
      sess.messages.push(userMsg)
      await appendMessage(sess.meta.id, userMsg)
      setMessages(prev => [...prev, { id: userMsg.id, role: 'user', text: userText }])
    }

    const sysPrompt = injectPrompt
      ? `${systemPrompt}\n\n---\n${injectPrompt}`
      : systemPrompt

    try {
      const thinkingOpts = thinkingEnabled
        ? { enabled: true as const, budgetTokens: thinkingBudget }
        : undefined

      const activeProvider = providerRef.current

      if (orchestratorMode) {
        await runOrchestratorTurn({
          provider: activeProvider,
          session: sess,
          tools: runtime.tools,
          bus: runtime.bus,
          permissions,
          systemPrompt: sysPrompt,
          catalog: modelCatalog,
          currentModel,
          signal: ac.signal,
        })
      } else {
        await runAgentTurn({
          provider: activeProvider,
          session: sess,
          tools: runtime.tools,
          bus: runtime.bus,
          permissions,
          systemPrompt: sysPrompt,
          signal: ac.signal,
          requestPermission,
          ...(thinkingOpts !== undefined ? { thinking: thinkingOpts } : {}),
        })
      }
    } catch (err) {
      if (!(err instanceof Error && err.name === 'AbortError')) {
        runtime.bus.emit({ type: 'agent:error', ts: Date.now(), sessionId: sess.meta.id, error: String(err) })
      }
      setBusy(false)
    } finally {
      abortRef.current = null
      setPendingPerm(null)
      permResolverRef.current = null
    }
  }, [permissions, runtime, systemPrompt, orchestratorMode, modelCatalog, currentModel, thinkingEnabled, thinkingBudget])

  const runCompact = useCallback(async () => {
    const sess = sessionRef.current
    if (sess.messages.length === 0) return

    setBusy(true)
    const ac = new AbortController()
    abortRef.current = ac

    try {
      const history = sess.messages.map(m => {
        const role = m.role === 'assistant' ? 'Assistant' : 'User'
        const text = m.content.map(blockToText).filter(Boolean).join('\n')
        return text ? `${role}:\n${text}` : null
      }).filter(Boolean).join('\n\n')

      const compactPrompt = `${COMPACT_PROMPT}\n\n<conversation>\n${history}\n</conversation>`

      let summary = ''
      for await (const event of providerRef.current.stream({
        model: sess.meta.model ?? 'claude-sonnet-4-6',
        messages: [{ role: 'user', content: [{ type: 'text', text: compactPrompt }] }],
        maxTokens: 4096,
      }, ac.signal)) {
        if (event.type === 'text_delta') {
          summary += event.delta
          setStreaming(summary)
        }
        if (ac.signal.aborted) break
      }

      setStreaming('')

      if (summary && !ac.signal.aborted) {
        const summaryMsg = makeMessage('user', `[Compact summary]\n${summary}`)
        sess.messages.splice(0, sess.messages.length, summaryMsg)
        await replaceMessages(sess.meta.id, sess.messages)
        setMessages([{
          id: summaryMsg.id, role: 'system',
          text: `Session compacted — history replaced with summary (${summary.length} chars).`,
        }])
      }
    } catch (err) {
      if (!(err instanceof Error && err.name === 'AbortError')) {
        setMessages(prev => [...prev, { id: randomUUID(), role: 'error', text: `Compact failed: ${String(err)}` }])
      }
    } finally {
      setBusy(false)
      abortRef.current = null
    }
  }, [])

  const onHistoryUp = useCallback(() => {
    const hist = historyRef.current
    if (hist.length === 0) return
    if (historyIdxRef.current === -1) {
      draftRef.current = inputValue  // save current draft
    }
    const next = Math.min(historyIdxRef.current + 1, hist.length - 1)
    historyIdxRef.current = next
    setInput(hist[hist.length - 1 - next]!)
  }, [inputValue])

  const onHistoryDown = useCallback(() => {
    const hist = historyRef.current
    if (historyIdxRef.current <= 0) {
      historyIdxRef.current = -1
      setInput(draftRef.current)
      return
    }
    const next = historyIdxRef.current - 1
    historyIdxRef.current = next
    setInput(hist[hist.length - 1 - next]!)
  }, [])

  const onSubmit = useCallback(async (raw: string) => {
    // Save to history
    if (raw.trim()) {
      historyRef.current.push(raw.trim())
    }
    historyIdxRef.current = -1
    draftRef.current = ''
    setInput('')
    const parsed = parseInput(raw)

    if (parsed.kind === 'command') {
      // Mode shortcuts
      if (parsed.name === 'build' || parsed.name === 'edit' || parsed.name === 'ask') {
        const next = parsed.name as ModeName
        permissions.setMode(next)
        setMode(next)
        setMessages(prev => [...prev, { id: randomUUID(), role: 'system', text: `Mode: ${next}` }])
        return
      }

      const ctx = {
        runtime,
        session: sessionRef.current,
        model: currentModel,
        setModel: switchModel,
        permissions,
        print: (text: string) => setMessages(prev => [...prev, { id: randomUUID(), role: 'system', text }]),
        clearSession: () => {
          sessionRef.current.messages.splice(0)
          setMessages([])
        },
        replaceSession: loadAndReplaceSession,
        exit: () => { void runtime.dispose().then(() => exit()) },
      }

      const result = await handleReplCommand(parsed.name, parsed.args, ctx)
      if (result.openModal === 'provider-setup') {
        setShowSetup(true)
        return
      }
      if (result.openModal === 'model-picker') {
        setShowModelPicker(true)
        return
      }
      if (result.openModal === 'session-picker') {
        setShowSessionPicker(true)
        return
      }
      if (result.setThinking !== undefined) {
        const { enabled, budget } = result.setThinking
        setThinking(enabled)
        setThinkingBudget(budget)
        setMessages(prev => [...prev, {
          id: randomUUID(), role: 'system',
          text: enabled
            ? `Extended thinking ON (budget: ${budget} tokens) — next turn will use reasoning.`
            : 'Extended thinking OFF.',
        }])
        return
      }
      if (result.compact) {
        await runCompact()
        return
      }
      if (result.reloadProvider) {
        const newModel = result.reloadProvider
        ctx.setModel(newModel)
        return
      }
      if (result.toggleOrchestrator !== undefined) {
        const next = !orchestratorMode
        setOrchestrator(next)
        if (next && modelCatalog.length === 0) {
          // Discover models in background — non-blocking
          void discoverModels(config, currentModel).then(catalog => setModelCatalog(catalog))
        }
        setMessages(prev => [...prev, {
          id: randomUUID(), role: 'system',
          text: next
            ? 'Orchestrator mode ON — discovering available models…'
            : 'Orchestrator mode OFF',
        }])
        return
      }
      if (result.output) {
        setMessages(prev => [...prev, { id: randomUUID(), role: 'system', text: result.output! }])
      }
      if (!result.handled) {
        setMessages(prev => [...prev, { id: randomUUID(), role: 'error', text: `Unknown command: /${parsed.name}  (type /help)` }])
      }
      if (result.injectPrompt) {
        await runTurn('', result.injectPrompt)
      }
      return
    }

    // Regular message with @mentions
    const expanded = await expandMentions(parsed.mentions, session.meta.cwd)
    const messageText = buildMessageWithMentions(parsed.text, expanded)
    await runTurn(messageText)
  }, [runtime, session, model, permissions, runTurn, exit])

  const onTabEmpty = useCallback(() => {
    const next = nextMode(mode)
    permissions.setMode(next)
    setMode(next)
  }, [mode, permissions])

  const handlePermAllow = useCallback(() => {
    if (!pendingPerm) return
    const resolve = pendingPerm.resolve
    setPendingPerm(null)
    resolve(true)
  }, [pendingPerm])

  const handlePermAllowAlways = useCallback(() => {
    if (!pendingPerm) return
    const resolve = pendingPerm.resolve
    setPendingPerm(null)
    resolve(true)
  }, [pendingPerm])

  const handlePermDeny = useCallback(() => {
    if (!pendingPerm) return
    const resolve = pendingPerm.resolve
    setPendingPerm(null)
    resolve(false)
  }, [pendingPerm])

  // Sync mode back if changed externally (e.g. /allow /deny)
  useEffect(() => { setMode(permissions.mode) }, [permissions.mode])

  const termWidth = stdout?.columns ?? 80
  const skillNames = runtime.skills ? runtime.skills.list().map(s => s.name) : []

  return (
    <Box flexDirection="column" width={termWidth}>
      {/* Provider setup overlay */}
      {showSetup && (
        <ProviderSetup
          onDone={(configured) => {
            setShowSetup(false)
            if (configured) {
              setMessages(prev => [...prev, {
                id: randomUUID(), role: 'system',
                text: 'Provider saved to settings.json — run /reload to apply.',
              }])
            }
          }}
        />
      )}

      {/* Model picker overlay */}
      {showModelPicker && (
        <ModelPicker
          currentModel={currentModel}
          onSelect={(m) => {
            setShowModelPicker(false)
            switchModel(m)
            setMessages(prev => [...prev, { id: randomUUID(), role: 'system', text: `Model: ${m}` }])
          }}
          onClose={() => setShowModelPicker(false)}
        />
      )}

      {/* Session picker overlay */}
      {showSessionPicker && (
        <SessionPicker
          onSelect={(sess) => {
            setShowSessionPicker(false)
            loadAndReplaceSession(sess)
          }}
          onClose={() => setShowSessionPicker(false)}
        />
      )}

      {/* Normal UI — hidden while any overlay is open */}
      {!showSetup && !showModelPicker && !showSessionPicker && (
        <>
          {/* Welcome banner — shown only before first message */}
          {messages.length === 0 && streamingText.length === 0 && (
            <WelcomeBanner model={currentModel} mode={mode} cwd={session.meta.cwd} />
          )}

          {/* Message area */}
          <MessageList messages={messages} streamingText={streamingText} reasoningText={reasoningText} />
        </>
      )}

      {/* Permission confirmation prompt */}
      {pendingPerm && (
        <PermissionPrompt
          callId={pendingPerm.callId}
          toolName={pendingPerm.toolName}
          input={pendingPerm.input}
          permissions={permissions}
          onAllow={handlePermAllow}
          onAllowAlways={handlePermAllowAlways}
          onDeny={handlePermDeny}
        />
      )}

      {/* Input — disabled while any overlay is open or agent is busy */}
      <PromptInput
        mode={mode}
        value={inputValue}
        onChange={setInput}
        onSubmit={onSubmit}
        onTabEmpty={onTabEmpty}
        onCtrlC={onCtrlC}
        disabled={busy || showSetup || showModelPicker || showSessionPicker}
        skillNames={skillNames}
        onHistoryUp={onHistoryUp}
        onHistoryDown={onHistoryDown}
      />

      {/* Status bar */}
      <StatusBar
        mode={mode}
        model={currentModel}
        tokens={tokens}
        contextWindow={CONTEXT_WINDOW}
        orchestrator={orchestratorMode}
        thinking={thinkingEnabled}
      />
    </Box>
  )
}
