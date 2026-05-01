import type { ModelConfig } from '../../api/types';
import type { BubbleChip } from '../../components/chat/ChatBubble';

// ===== Types =====

export type ChatMessage =
    | { type: 'user'; text: string; chips?: BubbleChip[] }
    | { type: 'assistant'; text: string }
    | { type: 'tool_call'; id: string; name: string; args: string; status: 'running' | 'done' | 'failed'; output?: string }
    | { type: 'error'; text: string; i18nKey?: string }
    | { type: 'cancelled'; text: string; i18nKey?: string };

export const MA_PAGE_SIZE = 30;

// ===== Context Type =====

export interface MotherAgentCtx {
    models: ModelConfig[];
    // conversation state
    agentModel: string | null;
    setAgentModel: (v: string | null) => void;
    chatInput: string;
    setChatInput: (v: string) => void;
    chatOutput: ChatMessage[];
    agentState: string;
    isProcessing: boolean;
    agentModelData: ModelConfig | undefined;
    chatInputFocused: boolean;
    setChatInputFocused: (v: boolean) => void;
    chatCursorPos: number;
    setChatCursorPos: (v: number) => void;
    chatInputRef: React.RefObject<HTMLInputElement>;
    chatEndRef: React.RefObject<HTMLDivElement>;
    logsEndRef: React.RefObject<HTMLDivElement>;
    handleSendLogsToAI: () => void;
    handleChatSend: () => void;
    sendMessage: (msg: string, displayText?: string, chips?: BubbleChip[]) => void;

    // ssh servers
    sshServers: Array<{ id: string; host: string; port: string; username: string; alias?: string }>;
    addSSHServer: (server: { id: string; host: string; port: string; username: string; password: string; alias?: string }) => void;
    removeSSHServer: (id: string) => void;
    selectedServerId: string;
    selectServer: (id: string) => void;
    clearChat: () => void;
    abortAgent: () => void;
    maDiskTotal: number;
    loadOlderChat: () => Promise<ChatMessage[]>;
}
