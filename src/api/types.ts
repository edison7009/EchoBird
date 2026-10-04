// Shared types for Tauri IPC — replaces window.electron types from vite-env.d.ts

// ─── Tool Types ───

export interface DetectedTool {
  id: string;
  name: string;
  category: string;
  installed: boolean;
  detectedPath?: string;
  configPath?: string;

  activeModel?: string;
  website?: string;
  apiProtocol?: string[];
  iconBase64?: string;
  names?: Record<string, string>;
  startCommand?: string;
  launchFile?: string;
  command?: string; // CLI install command (non-empty = installable via Mother Agent)
  version?: string; // Tool version from paths.json
  noModelConfig?: boolean; // Hide model picker; show "no model config supported" instead
  noCwdPrompt?: boolean; // Skip the folder picker on launch — run directly (CLI tools that don't need a working directory)
  launchUri?: string; // shell:AppsFolder\<AUMID> for MSIX / Store apps
}

// UI-level tool type used by AppManager, MotherAgent, and App shell
export interface LocalTool extends DetectedTool {
  path?: string;
  icon?: string;
  displayName?: string;
}

// ─── Model Types ───

export interface ModelConfig {
  internalId: string;
  name: string;
  modelId?: string;
  baseUrl: string;
  apiKey: string;
  anthropicUrl?: string;
  modelType?: 'CLOUD' | 'LOCAL' | 'TUNNEL' | 'DEMO';
  openaiTested?: boolean;
  anthropicTested?: boolean;
  openaiLatency?: number;
  anthropicLatency?: number;
  scope?: 'modelCenter' | 'smartRouter';
}

export interface ModelTestResult {
  success: boolean;
  latency: number;
  response?: string;
  error?: string;
  protocol: 'openai' | 'anthropic';
}

export interface PingResult {
  success: boolean;
  latency: number;
  url: string;
  error?: string;
}

export interface ToggleEncryptionResult {
  success: boolean;
  apiKey: string;
  encrypted: boolean;
}

// ─── Local LLM Types ───

export interface LocalServerInfo {
  running: boolean;
  port: number;
  modelName: string;
  pid?: number;
  apiKey: string;
}

export interface GgufFile {
  fileName: string;
  filePath: string;
  fileSize: number;
}

export interface HfModelEntry {
  modelName: string;
  modelPath: string;
  totalSize: number;
}

export interface ModelSettings {
  modelsDirs: string[];
  downloadDir?: string;
  gpuName?: string;
  gpuVramGb?: number;
}

// ─── Tool Config Types ───

export interface ToolModelInfo {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  model: string;
}

export interface ApplyModelInput {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  model: string;
  protocol?: string;
  /** Claude Desktop / Claude Code only. Connect directly to the selected
   * Anthropic-compatible relay instead of EchoBird's model-id router. */
  relayMode?: boolean;
  /** Codex CLI / ChatGPT desktop: enable live web search on model apply. */
  webSearch?: boolean;
  /**
   * Claude Code relay-only. When true (and `relayMode` is on), append `[1m]`
   * to the model id written to the 1M-capable env vars (`ANTHROPIC_MODEL` /
   * `ANTHROPIC_DEFAULT_SONNET_MODEL` / `ANTHROPIC_DEFAULT_OPUS_MODEL` / `ANTHROPIC_DEFAULT_FABLE_MODEL`) so
   * Claude Code budgets the 1M context window. Claude Code strips the suffix
   * before sending the id upstream, so the provider still sees the bare id.
   * `ANTHROPIC_DEFAULT_HAIKU_MODEL` and `CLAUDE_CODE_SUBAGENT_MODEL` never
   * get the suffix — no 1M concept. No effect in bridge mode (bridge writes
   * no model id — CC uses its built-in claude-* ids, which already budget the
   * full window). Claude Desktop uses this as prefer1m in either routing mode.
   * Other tools ignore this field.
   */
  oneMContext?: boolean;
}

// ─── App Settings Types ───

export interface AppSettings {
  locale?: string;
  themeMode?: 'light' | 'dark';
  colorTheme?: string;
  closeToTray?: boolean | null; // null = always ask, true = minimize to tray, false = quit directly
  closeWindowBehaviorSet?: boolean; // Track if user has made a choice about close behavior
}

// ─── Store Model Types ───

export interface StoreModelVariant {
  quantization: string;
  /** All GGUF files for this variant. Single-file variants list one entry;
   *  sharded multi-file GGUFs (e.g. unsloth `UD-Q4_K_XL/foo-00001-of-00010.gguf`
   *  … `…-00010-of-00010.gguf`) list every shard in order. The first entry's
   *  basename is the variant's primary key in the download progress map. */
  files: string[];
  /** Total bytes across all files in this variant. */
  fileSize: number;
  recommendedVRAM: string;
}

/** Normalize a store-model payload that may still carry the legacy
 *  `fileName: string` field instead of `files: string[]`. Applied at the
 *  data-entry point (after fetch_store_models) so internal code can rely
 *  on `variant.files` always being a non-empty array. Drops variants
 *  that have neither — they're malformed and can't be downloaded. */
export function normalizeStoreModels(raw: unknown): StoreModel[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .map((entry: unknown): StoreModel | null => {
      if (!entry || typeof entry !== 'object') return null;
      const m = entry as Record<string, unknown>;
      if (!Array.isArray(m.variants)) return null;
      const variants: StoreModelVariant[] = (m.variants as unknown[])
        .map((vEntry: unknown): StoreModelVariant | null => {
          if (!vEntry || typeof vEntry !== 'object') return null;
          const v = vEntry as Record<string, unknown>;
          const files: string[] | undefined =
            Array.isArray(v.files) && v.files.length
              ? (v.files as string[])
              : typeof v.fileName === 'string' && v.fileName
                ? [v.fileName]
                : undefined;
          if (!files) return null;
          return {
            quantization: String(v.quantization ?? ''),
            files,
            fileSize: Number(v.fileSize ?? 0),
            recommendedVRAM: String(v.recommendedVRAM ?? ''),
          };
        })
        .filter((v): v is StoreModelVariant => v !== null);
      if (!variants.length) return null;
      return {
        id: String(m.id ?? ''),
        name: String(m.name ?? ''),
        icon: String(m.icon ?? ''),
        description: String(m.description ?? ''),
        huggingfaceRepo: String(m.huggingfaceRepo ?? ''),
        modelScopeRepo: typeof m.modelScopeRepo === 'string' ? m.modelScopeRepo : undefined,
        runtimes: Array.isArray(m.runtimes) ? (m.runtimes as unknown[]).map(String) : undefined,
        variants,
      };
    })
    .filter((m): m is StoreModel => m !== null);
}

export interface StoreModel {
  id: string;
  name: string;
  icon: string;
  description: string;
  huggingfaceRepo: string;
  modelScopeRepo?: string;
  runtimes?: string[];
  variants: StoreModelVariant[];
}

// ─── Agent Types ───

export interface AgentRequest {
  message: string;
  model_id: string;
  base_url: string; // OpenAI-compatible URL (also used as final OpenAI fallback)
  api_key: string;
  model_name: string;
  provider: string;
  /** Anthropic-compatible URL. When provided, backend tries Anthropic first,
   *  falls back to OpenAI base_url on 400. */
  anthropic_url?: string;
  server_ids: string[];
  skills: string[];
  /** UI locale code (e.g. "zh-Hans", "en"). Hints the agent's response language. */
  locale?: string;
}

export type AgentEvent =
  | { type: 'text_delta'; text: string }
  | { type: 'thinking'; text: string }
  | { type: 'tool_call_start'; id: string; name: string }
  | { type: 'tool_call_args'; id: string; args: string }
  | { type: 'tool_result'; id: string; output: string; success: boolean }
  | { type: 'done' }
  | { type: 'error'; message: string }
  | { type: 'state'; state: string };

// ─── Parasite Types ───
// Mother Agent's "Connect" mode delegates a turn to the installed Claude
// Code CLI instead of running EchoBird's own agent_loop. The event shape
// mirrors AgentEvent so the chat UI can reuse the same renderer.

export interface ParasiteSendRequest {
  agentId: string;
  message: string;
}

export type ParasiteEvent =
  | { type: 'text_delta'; text: string }
  | { type: 'done' }
  | { type: 'error'; message: string }
  | { type: 'state'; state: string };
