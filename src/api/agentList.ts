// Shared agent definitions — single source of truth
// Used by: Channels (desktop), MobileApp (mobile)

export interface AgentDef {
    id: string;
    name: string;
    icon: string;
}

export const AGENT_LIST: AgentDef[] = [
    { id: 'openclaw', name: 'OpenClaw', icon: '/icons/tools/openclaw.svg' },
    { id: 'claudecode', name: 'Claude Code', icon: '/icons/tools/claudecode.svg' },
    { id: 'zeroclaw', name: 'ZeroClaw', icon: '/icons/tools/zeroclaw.png' },
    { id: 'nanobot', name: 'NanoBot', icon: '/icons/tools/nanobot.png' },
    { id: 'picoclaw', name: 'PicoClaw', icon: '/icons/tools/picoclaw.png' },
    { id: 'hermes', name: 'Hermes Agent', icon: '/icons/tools/hermes.png' },
];
