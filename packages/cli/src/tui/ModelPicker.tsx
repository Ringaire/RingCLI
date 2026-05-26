import React, { useState, useEffect } from 'react'
import { Box, Text, useInput } from 'ink'
import { loadConfig } from '@nekocode/core'
import { PRESETS } from '@nekocode/providers'

const VISIBLE = 12

interface Props {
  currentModel: string
  onSelect: (model: string) => void
  onClose: () => void
}

export function ModelPicker({ currentModel, onSelect, onClose }: Props) {
  const [models, setModels]       = useState<string[]>([])
  const [provider, setProvider]   = useState('')
  const [idx, setIdx]             = useState(0)
  const [scrollTop, setScrollTop] = useState(0)
  const [loading, setLoading]     = useState(true)

  useEffect(() => {
    void loadConfig().then(cfg => {
      const prov = (cfg.model ?? currentModel).split('/')[0] ?? 'anthropic'
      const cached = cfg.models?.[prov] ?? []
      setProvider(prov)
      setModels(cached)

      // Pre-select the currently active model
      const activeModelId = currentModel.includes('/')
        ? currentModel.split('/').slice(1).join('/')
        : currentModel
      const activeIdx = cached.findIndex(m => m === activeModelId)
      if (activeIdx >= 0) {
        setIdx(activeIdx)
        setScrollTop(Math.max(0, activeIdx - Math.floor(VISIBLE / 2)))
      }
      setLoading(false)
    })
  }, [currentModel])

  useInput((input, key) => {
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
        const next = Math.min(models.length - 1, i + 1)
        setScrollTop(t => next >= t + VISIBLE ? next - VISIBLE + 1 : t)
        return next
      })
      return
    }
    if (key.return) {
      const selected = models[idx]
      if (selected) onSelect(`${provider}/${selected}`)
      return
    }

    void input
  })

  if (loading) {
    return (
      <Box paddingX={2} paddingY={1}>
        <Text dimColor>Loading model list…</Text>
      </Box>
    )
  }

  if (models.length === 0) {
    const preset = PRESETS[provider]
    return (
      <Box flexDirection="column" paddingX={2} paddingY={1}>
        <Box gap={2} marginBottom={1}>
          <Text bold color="cyan">Switch Model  —  {provider}</Text>
        </Box>
        <Text dimColor>No cached models for <Text color="yellow">{provider}</Text>.</Text>
        {preset?.baseUrl && (
          <Text dimColor>Run <Text color="cyan">/model refresh</Text> to fetch from {preset.baseUrl}</Text>
        )}
        <Box marginTop={1}><Text dimColor>Esc to cancel</Text></Box>
      </Box>
    )
  }

  const visible = models.slice(scrollTop, scrollTop + VISIBLE)
  const activeModelId = currentModel.includes('/')
    ? currentModel.split('/').slice(1).join('/')
    : currentModel

  return (
    <Box flexDirection="column" paddingX={2} paddingY={1}>
      <Box gap={2} marginBottom={1}>
        <Text bold color="cyan">Switch Model  —  {provider}</Text>
        <Text dimColor>{models.length} models  •  ↑↓ navigate  Enter select  Esc cancel</Text>
      </Box>
      <Box flexDirection="column">
        {scrollTop > 0 && <Text dimColor>  ↑ {scrollTop} more</Text>}
        {visible.map((m, i) => {
          const absIdx = scrollTop + i
          const sel    = absIdx === idx
          const active = m === activeModelId
          const marker = active ? ' ◀' : ''
          return (
            <Box key={m}>
              {sel
                ? <Text color="cyan" bold>{'› '}{m}{marker}</Text>
                : <Text dimColor>{'  '}{m}{active ? <Text color="green">{marker}</Text> : ''}</Text>
              }
            </Box>
          )
        })}
        {scrollTop + VISIBLE < models.length && (
          <Text dimColor>  ↓ {models.length - scrollTop - VISIBLE} more</Text>
        )}
      </Box>
    </Box>
  )
}
