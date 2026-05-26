import React, { useState, useEffect } from 'react'
import { Box, Text, useInput } from 'ink'
import { listSessions, loadSession } from '@nekocode/core'
import type { Session, SessionMeta } from '@nekocode/core'

const VISIBLE = 12

interface Props {
  onSelect: (session: Session) => void
  onClose: () => void
}

function timeAgo(ts: number): string {
  const age = Date.now() - ts
  if (age < 60_000)       return 'just now'
  if (age < 3_600_000)    return `${Math.floor(age / 60_000)}m ago`
  if (age < 86_400_000)   return `${Math.floor(age / 3_600_000)}h ago`
  return `${Math.floor(age / 86_400_000)}d ago`
}

export function SessionPicker({ onSelect, onClose }: Props) {
  const [sessions, setSessions]   = useState<SessionMeta[]>([])
  const [idx, setIdx]             = useState(0)
  const [scrollTop, setScrollTop] = useState(0)
  const [loading, setLoading]     = useState(true)
  const [loadingId, setLoadingId] = useState<string | null>(null)
  const [error, setError]         = useState<string | null>(null)

  useEffect(() => {
    void listSessions().then(all => {
      setSessions(all)
      setLoading(false)
    })
  }, [])

  useInput((input, key) => {
    if (loadingId) return  // waiting for session load

    if (key.escape) { onClose(); return }

    if (key.upArrow) {
      setIdx(i => {
        const next = Math.max(0, i - 1)
        setScrollTop(t => next < t ? next : t)
        return next
      })
      return
    }
    if (key.downArrow) {
      setIdx(i => {
        const next = Math.min(sessions.length - 1, i + 1)
        setScrollTop(t => next >= t + VISIBLE ? next - VISIBLE + 1 : t)
        return next
      })
      return
    }
    if (key.return) {
      const meta = sessions[idx]
      if (!meta) return
      setLoadingId(meta.id)
      setError(null)
      void loadSession(meta.id).then(sess => {
        if (!sess) { setError(`Failed to load session ${meta.id.slice(0, 8)}`); setLoadingId(null); return }
        onSelect(sess)
      })
      return
    }

    void input
  })

  if (loading) {
    return (
      <Box paddingX={2} paddingY={1}>
        <Text dimColor>Loading sessions…</Text>
      </Box>
    )
  }

  if (loadingId) {
    return (
      <Box paddingX={2} paddingY={1}>
        <Text dimColor>Loading session {loadingId.slice(0, 8)}…</Text>
      </Box>
    )
  }

  if (sessions.length === 0) {
    return (
      <Box flexDirection="column" paddingX={2} paddingY={1}>
        <Text bold color="cyan">Sessions</Text>
        <Text dimColor>No saved sessions yet.</Text>
        <Box marginTop={1}><Text dimColor>Esc to cancel</Text></Box>
      </Box>
    )
  }

  const visible = sessions.slice(scrollTop, scrollTop + VISIBLE)

  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Box gap={2} marginBottom={1}>
        <Text bold color="cyan">Sessions</Text>
        <Text dimColor>{sessions.length} saved  •  ↑↓ navigate  Enter load  Esc cancel</Text>
      </Box>
      {error && <Text color="red">{error}</Text>}
      <Box flexDirection="column">
        {scrollTop > 0 && <Text dimColor>  ↑ {scrollTop} more</Text>}
        {visible.map((m, i) => {
          const absIdx = scrollTop + i
          const sel    = absIdx === idx
          const title  = (m.title ?? '(no title)').slice(0, 36).padEnd(36)
          const msgs   = String(m.messageCount ?? 0).padStart(3)
          const age    = timeAgo(m.updatedAt)
          return (
            <Box key={m.id}>
              {sel
                ? <Text color="cyan" bold>{'› '}{m.id.slice(0, 8)}  {title}  {msgs} msgs  {age}</Text>
                : <Text dimColor>{'  '}{m.id.slice(0, 8)}  {title}  {msgs} msgs  {age}</Text>
              }
            </Box>
          )
        })}
        {scrollTop + VISIBLE < sessions.length && (
          <Text dimColor>  ↓ {sessions.length - scrollTop - VISIBLE} more</Text>
        )}
      </Box>
    </Box>
  )
}
