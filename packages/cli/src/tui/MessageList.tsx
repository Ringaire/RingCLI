import React, { useState } from 'react'
import { Box, Text, useInput } from 'ink'
import { renderMarkdown } from './markdown.js'
import { extractToolPreview } from '../agent/tool-preview.js'

export type MessageRole = 'user' | 'assistant' | 'tool' | 'error' | 'system' | 'reasoning'

export interface DisplayMessage {
  id: string
  role: MessageRole
  text: string
  /** For tool messages: the tool name */
  toolName?: string
  /** Raw tool input — used to extract a human-readable preview */
  toolInput?: unknown
  /** Duration in ms for completed tool calls */
  durationMs?: number
  ok?: boolean
}

interface Props {
  messages: DisplayMessage[]
  /** Streaming text being accumulated (not yet a full message) */
  streamingText: string
  /** Streaming reasoning tokens (empty when not actively thinking) */
  reasoningText: string
}

// ── Spinner frames for live indicators ───────────────────────────────────────

const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']

function useSpinner(): string {
  const [frame, setFrame] = React.useState(0)
  React.useEffect(() => {
    const id = setInterval(() => setFrame(f => (f + 1) % SPINNER.length), 80)
    return () => clearInterval(id)
  }, [])
  return SPINNER[frame] ?? '⠋'
}

// ── Message components ────────────────────────────────────────────────────────

function UserMsg({ text }: { text: string }) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text color="cyan" bold>user</Text>
      <Box paddingLeft={2}>
        <Text wrap="wrap">{text}</Text>
      </Box>
    </Box>
  )
}

function AssistantMsg({ text }: { text: string }) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text color="green" bold>assistant</Text>
      <Box paddingLeft={2} flexDirection="column">
        <Text wrap="wrap">{renderMarkdown(text)}</Text>
      </Box>
    </Box>
  )
}

function ToolMsg({ toolName, toolInput, ok, durationMs }: {
  toolName: string
  toolInput?: unknown
  ok?: boolean | undefined
  durationMs?: number | undefined
}) {
  const status = ok === undefined ? '…' : ok ? 'ok' : 'err'
  const statusColor = ok === undefined ? undefined : ok ? 'green' : 'red'
  const dur = durationMs !== undefined ? ` ${durationMs}ms` : ''
  const preview = toolInput !== undefined ? extractToolPreview(toolName, toolInput).summary : ''
  return (
    <Box paddingLeft={2} marginBottom={0} flexDirection="column">
      <Box>
        <Text dimColor>[tool] </Text>
        <Text color="yellow">{toolName}</Text>
        <Text dimColor> </Text>
        {statusColor !== undefined
          ? <Text color={statusColor}>[{status}]</Text>
          : <Text dimColor>[{status}]</Text>
        }
        <Text dimColor>{dur}</Text>
      </Box>
      {preview !== '' && (
        <Box paddingLeft={7}>
          <Text dimColor wrap="truncate-end">{preview}</Text>
        </Box>
      )}
    </Box>
  )
}

function ErrorMsg({ text }: { text: string }) {
  return (
    <Box flexDirection="column" marginBottom={1} paddingLeft={2}>
      <Text color="red">[error] {text}</Text>
    </Box>
  )
}

function SystemMsg({ text }: { text: string }) {
  return (
    <Box marginBottom={1}>
      <Text dimColor italic>[{text}]</Text>
    </Box>
  )
}

// Reasoning block — collapsible, press T to toggle
function ReasoningMsg({ text, id }: { text: string; id: string }) {
  const [expanded, setExpanded] = useState(false)

  useInput((input, key) => {
    // Toggle on 't' when not in input (rough focus check — always active for now)
    if (input === 't' && !key.ctrl && !key.meta) {
      setExpanded(e => !e)
    }
  })

  const lines = text.split('\n').length
  const chars = text.length
  const preview = expanded ? null : text.slice(0, 120).replace(/\n/g, ' ')

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box gap={1}>
        <Text color="magenta" dimColor>
          {expanded ? '▾' : '▸'} Reasoning
        </Text>
        <Text dimColor>({lines} lines, {chars} chars)</Text>
        <Text dimColor italic>— press T to {expanded ? 'collapse' : 'expand'}</Text>
      </Box>
      {expanded ? (
        <Box paddingLeft={2} flexDirection="column">
          <Text dimColor wrap="wrap">{text}</Text>
        </Box>
      ) : (
        <Box paddingLeft={2}>
          <Text dimColor wrap="wrap">{preview}…</Text>
        </Box>
      )}
    </Box>
  )
}

// Streaming reasoning indicator
function ReasoningStream({ text }: { text: string }) {
  const spin = useSpinner()
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Box gap={1}>
        <Text color="magenta" dimColor>{spin} Reasoning…</Text>
        <Text dimColor>({text.length} chars)</Text>
      </Box>
    </Box>
  )
}

// Streaming assistant indicator
function AssistantStream({ text }: { text: string }) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text color="green" bold>assistant</Text>
      <Box paddingLeft={2}>
        <Text wrap="wrap">{text}</Text>
      </Box>
    </Box>
  )
}

// ── Main list ─────────────────────────────────────────────────────────────────

export function MessageList({ messages, streamingText, reasoningText }: Props) {
  return (
    <Box flexDirection="column" flexGrow={1} paddingX={1} paddingTop={1}>
      {messages.map((m) => {
        switch (m.role) {
          case 'user':      return <UserMsg      key={m.id} text={m.text} />
          case 'assistant': return <AssistantMsg key={m.id} text={m.text} />
          case 'tool':      return <ToolMsg      key={m.id} toolName={m.toolName ?? m.text} toolInput={m.toolInput} ok={m.ok} durationMs={m.durationMs} />
          case 'error':     return <ErrorMsg     key={m.id} text={m.text} />
          case 'system':    return <SystemMsg    key={m.id} text={m.text} />
          case 'reasoning': return <ReasoningMsg key={m.id} id={m.id} text={m.text} />
        }
      })}

      {/* Live streaming reasoning (before the text starts) */}
      {reasoningText.length > 0 && streamingText.length === 0 && (
        <ReasoningStream text={reasoningText} />
      )}

      {/* Live streaming assistant text */}
      {streamingText.length > 0 && (
        <AssistantStream text={streamingText} />
      )}
    </Box>
  )
}
