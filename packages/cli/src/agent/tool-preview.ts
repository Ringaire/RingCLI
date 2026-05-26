export interface ToolPreview {
  /** Single-line summary shown in tool message and permission prompt header */
  summary: string
  /** Extra lines shown in permission prompt (e.g. diff, content preview) */
  detail?: string | undefined
}

export function extractToolPreview(toolName: string, input: unknown): ToolPreview {
  if (!input || typeof input !== 'object') return { summary: '' }
  const i = input as Record<string, unknown>

  switch (toolName) {
    case 'bash':
    case 'run_command': {
      const cmd = typeof i['command'] === 'string' ? i['command'] : ''
      const lines = cmd.split('\n')
      const summary = (lines[0] ?? '').slice(0, 80)
      const detail = lines.length > 1 ? lines.slice(1, 4).join('\n') : undefined
      return { summary, detail }
    }
    case 'write_file': {
      const path = typeof i['path'] === 'string' ? i['path'] : ''
      const content = typeof i['content'] === 'string' ? i['content'] : ''
      const preview = content.split('\n').slice(0, 4).join('\n')
      return { summary: path, detail: preview || undefined }
    }
    case 'edit_file': {
      const path = typeof i['path'] === 'string' ? i['path'] : ''
      const old = typeof i['old_string'] === 'string' ? i['old_string'] : ''
      const neu = typeof i['new_string'] === 'string' ? i['new_string'] : ''
      const detail = old ? `- ${old.split('\n')[0]?.slice(0, 60)}\n+ ${neu.split('\n')[0]?.slice(0, 60)}` : undefined
      return { summary: path, detail }
    }
    case 'read_file':
      return { summary: typeof i['path'] === 'string' ? i['path'] : '' }
    case 'glob':
      return { summary: typeof i['pattern'] === 'string' ? i['pattern'] : '' }
    case 'grep':
      return { summary: typeof i['pattern'] === 'string' ? i['pattern'] : '' }
    case 'web_fetch':
      return { summary: typeof i['url'] === 'string' ? i['url'].slice(0, 80) : '' }
    case 'web_search':
      return { summary: typeof i['query'] === 'string' ? i['query'].slice(0, 80) : '' }
    default:
      return { summary: '' }
  }
}
