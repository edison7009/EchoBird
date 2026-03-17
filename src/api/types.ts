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
    command?: string;        // CLI install command (non-empty = installable via Mother Agent)
    version?: string;        // Tool version from paths.json
}

// UI-level tool type used by AppManager, MotherAgent, and App shell
export interface LocalTool extends DetectedTool {
    path?: string;
    icon?: string;

}

// ─── Model Types ───

export interface SSNodeConfig {
    name: string;
    server: string;
    port: number;
    cipher: string;
    password: string;
}

export interface ModelConfig {
    internalId: string;
    name: string;
    modelId?: string;
    baseUrl: string;
    apiKey: string;
    anthropicUrl?: string;
    modelType?: 'CLOUD' | 'LOCAL' | 'TUNNEL' | 'DEMO';
    proxyUrl?: string;
    ssNode?: SSNodeConfig;
    openaiTested?: boolean;
    anthropicTested?: boolean;
    openaiLatency?: number;
    anthropicLatency?: number;
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

// ─── Proxy Types ───

export interface ProxyRule {
    pattern: string;
    enabled: boolean;
}

// ─── Tool Config Types ───

export interface ToolModelInfo {
    id: string;
    name: string;
    baseUrl: string;
    apiKey: string;
    model: string;
    proxyUrl?: string;
}

export interface ApplyModelInput {
    id: string;
    name: string;
    baseUrl: string;
    apiKey: string;
    model: string;
    proxyUrl?: string;
    protocol?: string;
}

// ─── App Log Types ───

export interface AppLogEntry {
    timestamp: string;
    category: string;
    message: string;
}

// ─── Channel Types ───

export interface ChannelConfig {
    id: number;
    name: string;
    protocol: string;
    address: string;
}

// ─── App Settings Types ───

export interface AppSettings {
    locale?: string;
    closeBehavior?: string;
}

// ─── Store Model Types ───

export interface StoreModelVariant {
    quantization: string;
    fileName: string;
    fileSize: number;
    recommendedVRAM: string;
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
    base_url: string;       // OpenAI-compatible URL (also used as final OpenAI fallback)
    api_key: string;
    model_name: string;
    provider: string;
    /** Anthropic-compatible URL. When provided, backend tries Anthropic first,
     *  falls back to OpenAI base_url on 400. */
    anthropic_url?: string;
    proxy_url?: string;
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

export interface AgentStatusResponse {
    session_id: string;
    running: boolean;
    message_count: number;
}
