import React from 'react'
import { Box, Text, useInput } from 'ink'
import { extractToolPreview } from '../agent/tool-preview.js'
import type { DefaultPermissionEngine } from '@nekocode/core/permissions'

interface Props {
  callId: string
  toolName: string
  input: unknown
  permissions: DefaultPermissionEngine
  onAllow: () => void
  onAllowAlways: () => void
  onDeny: () => void
}

export function PermissionPrompt({ toolName, input, permissions, onAllow, onAllowAlways, onDeny }: Props) {
  const preview = extractToolPreview(toolName, input)

  useInput((ch) => {
    if (ch === '1') { onAllow() }
    else if (ch === '2') { permissions.allow(toolName); onAllowAlways() }
    else if (ch === '3') { onDeny() }
  })

  return (
    <Box
      flexDirection="column"
      borderStyle="single"
      borderColor="yellow"
      paddingX={1}
      paddingY={0}
      marginY={1}
    >
      <Box gap={1} marginBottom={preview.summary !== '' || preview.detail !== undefined ? 0 : 0}>
        <Text color="yellow" bold>NekoCode wants to use a tool:</Text>
        <Text color="cyan" bold>{toolName}</Text>
      </Box>

      {preview.summary !== '' && (
        <Box paddingLeft={2}>
          <Text dimColor wrap="truncate-end">{preview.summary}</Text>
        </Box>
      )}
      {preview.detail !== undefined && (
        <Box paddingLeft={2} flexDirection="column">
          {preview.detail.split('\n').map((line, i) => (
            <Text key={i} dimColor>{line}</Text>
          ))}
        </Box>
      )}

      <Box flexDirection="column" marginTop={1}>
        <Text>
          <Text color="green" bold>1</Text>
          <Text dimColor> Allow once</Text>
        </Text>
        <Text>
          <Text color="green" bold>2</Text>
          <Text dimColor> Always allow for this session</Text>
        </Text>
        <Text>
          <Text color="red" bold>3</Text>
          <Text dimColor> Deny</Text>
        </Text>
      </Box>
    </Box>
  )
}
