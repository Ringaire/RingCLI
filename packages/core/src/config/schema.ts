import type { McpServerConfig } from '../runtime/types.js'

/** Legacy single-provider config (kept for backwards compatibility) */
export interface ProviderConfig {
  type: 'anthropic' | 'openai' | 'gemini' | 'ollama' | 'openai-compatible'
  apiKey?: string
  baseUrl?: string
  model?: string
}

/** Entry for the named providers map */
export interface ProviderEntry {
  /** Provider type — required for custom providers not in built-in presets */
  type?: 'anthropic' | 'openai' | 'gemini' | 'openai-compatible'
  apiKey?: string
  /** Base URL — required for custom providers, optional override for presets */
  baseUrl?: string
}

export interface NekoUserConfig {
  /**
   * Named providers keyed by preset name (anthropic, openai, deepseek, …).
   * Only apiKey (and optional baseUrl override) needed — base URLs are built-in.
   *
   * Example:
   *   { "anthropic": { "apiKey": "sk-ant-…" }, "deepseek": { "apiKey": "sk-…" } }
   */
  providers?: Record<string, ProviderEntry>

  /**
   * Active model in "provider/model-id" format.
   * Example: "anthropic/claude-sonnet-4-6", "deepseek/deepseek-chat"
   */
  model?: string

  /**
   * Cached model lists per provider. Populated on first /connect.
   * Key is provider name, value is array of model IDs.
   *
   * Example:
   *   { "anthropic": ["claude-sonnet-4-6", "claude-opus-4-7", ...] }
   */
  models?: Record<string, string[]>

  /** HTTP proxy URL applied to all outbound requests, e.g. "http://127.0.0.1:7890" */
  proxy?: string

  /** @deprecated Use providers + model instead */
  provider?: ProviderConfig

  /** MCP servers keyed by name */
  mcpServers?: Record<string, McpServerConfig>

  session?: {
    maxMessages?: number
    maxTokens?: number
    autoSaveMs?: number
  }

  ui?: {
    theme?: 'dark' | 'light' | 'auto'
    compactMode?: boolean
    showTokenCount?: boolean
  }
}

/** All fields resolved (proxy / providers / model remain optional) */
export type ResolvedConfig = Required<Omit<NekoUserConfig, 'proxy' | 'providers' | 'model' | 'provider'>> & Pick<NekoUserConfig, 'proxy' | 'providers' | 'model' | 'provider'>

export const defaultConfig: ResolvedConfig = {
  providers: {},
  model: 'anthropic/claude-sonnet-4-6',
  models: {},
  mcpServers: {},
  session: {
    maxMessages: 200,
    maxTokens: 180_000,
    autoSaveMs: 0,
  },
  ui: {
    theme: 'auto',
    compactMode: false,
    showTokenCount: true,
  },
}
