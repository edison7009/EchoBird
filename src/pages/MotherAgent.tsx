import React, { useState, useEffect, useRef, useCallback, createContext, useContext } from 'react';
import { Paperclip, ImageIcon, KeyRound, Send, X, ChevronDown, Square, Lock, RotateCcw, ChevronLeft, ChevronRight, ChevronsDown, Globe, Info, CheckCircle, HelpCircle, ChevronUp, Code, Search, X as XIcon, FileText, Sparkles, Plus, Bot, Database, Settings2 } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { MiniSelect } from '../components/MiniSelect';
import { getModelIcon } from '../components/cards/ModelCard';
import { PendingChipsRow } from '../components/PendingChipsRow';
import { ChatBubble, type BubbleChip } from '../components/chat';
import { errorToKey } from '../utils/normalizeError';
import { buildPendingMessage } from '../utils/buildPendingMessage';
import { useI18n } from '../hooks/useI18n';
import { useConfirm } from '../components/ConfirmDialog';
import * as api from '../api/tauri';
import { channelHistoryLoad } from '../api/tauri';
import type { ModelConfig, LocalTool, AppLogEntry, AgentEvent } from '../api/types';
import { useChatPersistence } from '../hooks/useChatPersistence';
import type { DiskMsg } from '../hooks/useChatPersistence';

// Markdown components config (Mother Agent blue theme)
const mdComponents = {
    code: ({ className, children, ...props }: any) => {
        const isInline = !className;
        return isInline ? (
            <code className="bg-cyber-accent-secondary/10 text-cyber-accent-secondary px-1.5 py-0.5 rounded text-[0.85em] font-mono" {...props}>{children}</code>
        ) : (
            <code className={`block bg-black/40 border border-cyber-border/30 rounded-lg p-3 pr-10 my-2 text-[0.85em] font-mono text-cyber-text-primary overflow-x-auto whitespace-pre ${className || ''}`} {...props}>{children}</code>
        );
    },
    pre: ({ children }: any) => {
        const codeText = String(children?.props?.children || '').replace(/\n$/, '');
        const isBlock = children?.props?.className;
        if (!isBlock) return <>{children}</>;
        return (
            <div className="relative group">
                {children}
                <button
                    onClick={(e: React.MouseEvent<HTMLButtonElement>) => {
                        navigator.clipboard.writeText(codeText);
                        const btn = e.currentTarget;
                        const svg = btn.querySelector('svg');
                        if (svg) svg.style.display = 'none';
                        btn.insertAdjacentHTML('beforeend', '<span class="copy-ok" style="color:#7aa2f7">✓</span>');
                        setTimeout(() => {
                            const ok = btn.querySelector('.copy-ok');
                            if (ok) ok.remove();
                            if (svg) svg.style.display = '';
                        }, 1500);
                    }}
                    className="absolute top-2 right-2 w-6 h-6 flex items-center justify-center text-cyber-text-muted/40 hover:text-cyber-accent-secondary transition-colors"
                ><svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" /></svg></button>
            </div>
        );
    },
    a: ({ href, children }: any) => (
        <a href={href} className="text-cyber-accent-secondary hover:underline" onClick={(e: React.MouseEvent) => { e.preventDefault(); api.openExternal(href); }}>{children}</a>
    ),
    strong: ({ children }: any) => <strong className="text-cyber-text-primary font-bold">{children}</strong>,
    em: ({ children }: any) => <em className="text-cyber-accent-secondary/80">{children}</em>,
    ul: ({ children }: any) => <ul className="list-disc list-inside my-1 space-y-0.5">{children}</ul>,
    ol: ({ children }: any) => <ol className="list-decimal list-inside my-1 space-y-0.5">{children}</ol>,
    h1: ({ children }: any) => <h1 className="text-lg font-bold text-cyber-text-primary mt-3 mb-1">{children}</h1>,
    h2: ({ children }: any) => <h2 className="text-base font-bold text-cyber-text-primary mt-2 mb-1">{children}</h2>,
    h3: ({ children }: any) => <h3 className="text-sm font-bold text-cyber-text-primary mt-2 mb-1">{children}</h3>,
    blockquote: ({ children }: any) => <blockquote className="border-l-2 border-cyber-accent-secondary/40 pl-3 my-1 text-cyber-text-muted/60 italic">{children}</blockquote>,
    hr: () => <hr className="border-cyber-border/30 my-2" />,
    table: ({ children }: any) => <table className="border-collapse my-2 text-sm w-full">{children}</table>,
    th: ({ children }: any) => <th className="border border-cyber-border/30 px-2 py-1 bg-cyber-accent-secondary/5 text-left font-bold">{children}</th>,
    td: ({ children }: any) => <td className="border border-cyber-border/30 px-2 py-1">{children}</td>,
};

declare const __APP_VERSION__: string;

// ===== Types =====

export type ChatMessage =
    | { type: 'user'; text: string; chips?: import('../components/chat/ChatBubble').BubbleChip[] }
    | { type: 'assistant'; text: string }
    | { type: 'thinking'; text: string }
    | { type: 'tool_call'; id: string; name: string; args: string; status: 'running' | 'done' }
    | { type: 'tool_result'; id: string; output: string; success: boolean }
    | { type: 'error'; text: string; i18nKey?: string }
    | { type: 'cancelled'; text: string; i18nKey?: string }
    | { type: 'state'; state: string };

const MA_PAGE_SIZE = 30;

// ===== Context (shared state between Main & Panel) =====

interface MotherAgentCtx {
    appLogs: AppLogEntry[];
    models: ModelConfig[];
    detectedTools: LocalTool[];
    onClearLogs: () => void;
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
    sendMessage: (msg: string, displayText?: string, chips?: import('../components/chat/ChatBubble').BubbleChip[]) => void;

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

const MotherAgentContext = createContext<MotherAgentCtx | null>(null);
const useMotherAgent = () => {
    const ctx = useContext(MotherAgentContext);
    if (!ctx) throw new Error('useMotherAgent must be used within MotherAgentProvider');
    return ctx;
};

// ===== Provider =====
interface MotherAgentProviderProps {
    appLogs: AppLogEntry[];
    detectedTools: LocalTool[];
    onClearLogs: () => void;
    onAgentRunningChange?: (running: boolean) => void;
    onNewMessage?: () => void;
    initialMessage?: string;
    children: React.ReactNode;
}

export function MotherAgentProvider({ appLogs, detectedTools, onClearLogs, onAgentRunningChange, onNewMessage, initialMessage, children }: MotherAgentProviderProps) {
    const { t, locale } = useI18n();  // locale for agent hint; t for error messages
    const [models, setModels] = useState<ModelConfig[]>([]);
    const [agentModel, setAgentModelRaw] = useState<string | null>(() => localStorage.getItem('echobird_agent_model'));
    const setAgentModel = useCallback((v: string | null) => {
        setAgentModelRaw(v);
        if (v) localStorage.setItem('echobird_agent_model', v);
        else localStorage.removeItem('echobird_agent_model');
    }, []);
    const [chatInput, setChatInput] = useState('');
    const [chatOutput, setChatOutput] = useState<ChatMessage[]>([]);
    // Per-server chat history map
    const chatHistoryMap = useRef<Map<string, ChatMessage[]>>(new Map());

    const [isProcessing, setIsProcessing] = useState(false);
    const [agentState, setAgentState] = useState('idle');
    const [chatInputFocused, setChatInputFocused] = useState(false);
    const [chatCursorPos, setChatCursorPos] = useState(0);
    const logsEndRef = useRef<HTMLDivElement>(null!);
    const chatEndRef = useRef<HTMLDivElement>(null!);
    const chatInputRef = useRef<HTMLInputElement>(null!);

    useEffect(() => {
        if (!initialMessage) return;
        setChatInput(initialMessage);
        setTimeout(() => chatInputRef.current?.focus(), 100);
    }, [initialMessage]);

    // SSH servers shared state (persisted via backend)
    const [sshServers, setSSHServers] = useState<Array<{ id: string; host: string; port: string; username: string; alias?: string }>>([]);

    // Load saved SSH servers on mount
    useEffect(() => {
        api.loadSSHServers().then(servers => {
            setSSHServers(servers.map(s => ({
                id: s.id,
                host: s.host,
                port: String(s.port),
                username: s.username,
                alias: s.alias,
            })));
        }).catch(() => { });
    }, []);

    const addSSHServer = useCallback(async (server: { id: string; host: string; port: string; username: string; password?: string; alias?: string }) => {
        setSSHServers(prev => [...prev, { id: server.id, host: server.host, port: server.port, username: server.username, alias: server.alias }]);
        await api.saveSSHServer(server.id, server.host, parseInt(server.port) || 22, server.username, server.password || '', server.alias).catch(() => { });
        window.dispatchEvent(new Event('ssh-servers-changed'));
    }, []);
    const removeSSHServer = useCallback(async (id: string) => {
        setSSHServers(prev => prev.filter(s => s.id !== id));
        setSelectedServerId(prev => prev === id ? 'local' : prev);
        // Remove from backend
        await api.removeSSHServerFromDisk(id).catch(() => { });
        window.dispatchEvent(new Event('ssh-servers-changed'));
    }, []);

    // Server selection (single-select)
    const [selectedServerId, setSelectedServerId] = useState('local');
    // Red pulse trigger — only fires for genuinely NEW errors, resets on server switch
    const prevErrCountRef = useRef(0);
    const prevServerIdRef = useRef('');
    useEffect(() => {
        const errCount = chatOutput.filter(m => m.type === 'error').length;
        if (selectedServerId !== prevServerIdRef.current) {
            // Server switched — sync without firing
            prevErrCountRef.current = errCount;
            prevServerIdRef.current = selectedServerId;
        } else if (errCount > prevErrCountRef.current) {
            window.dispatchEvent(new CustomEvent('chat-error', { detail: { count: errCount - prevErrCountRef.current } }));
            prevErrCountRef.current = errCount;
        }
    }, [chatOutput, selectedServerId]);
    const prevServerRef = useRef('local');
    const agentChatKey = (id: string) => `agent_${id}`;

    // Shared mapper: ChatMessage → disk format
    const toDisk = useCallback((m: ChatMessage): DiskMsg | null => {
        if (m.type === 'user') return { role: 'user', content: m.text };
        if (m.type === 'assistant') return { role: 'assistant', content: m.text };
        if (m.type === 'error') return { role: 'system', content: (m as any).i18nKey || m.text };
        if (m.type === 'cancelled') return { role: 'system', content: (m as any).i18nKey || m.text };
        return null; // skip thinking, tool_call, tool_result, state
    }, []);

    // Shared mapper: disk format → ChatMessage
    const fromDisk = useCallback((m: DiskMsg): ChatMessage => {
        if (m.role === 'system' && m.content === 'error.userCancelled') {
            return { type: 'cancelled', text: '', i18nKey: m.content };
        }
        if (m.role === 'system' && m.content.startsWith('error.')) {
            return { type: 'error', text: '', i18nKey: m.content };
        }
        if (m.role === 'system') {
            return { type: 'cancelled', text: m.content };
        }
        return {
            type: m.role === 'user' ? 'user' : 'assistant',
            text: m.content,
        } as ChatMessage;
    }, []);

    const prependMessages = useCallback((older: ChatMessage[]) => {
        setChatOutput(prev => [...older, ...prev]);
    }, []);

    const setMessagesFromDisk = useCallback((msgs: ChatMessage[]) => {
        chatHistoryMap.current.set('local', msgs);
        setChatOutput(msgs);
    }, []);

    const persistence = useChatPersistence<ChatMessage>({
        diskKey: agentChatKey(selectedServerId),
        messages: chatOutput,
        prependMessages,
        setMessages: setMessagesFromDisk,
        toDisk,
        fromDisk,
        pageSize: MA_PAGE_SIZE,
    });

    // Load chat history from disk on mount
    useEffect(() => { persistence.loadInitial(); }, []);

    const selectServer = useCallback(async (id: string) => {
        // Save current chat to history map
        chatHistoryMap.current.set(prevServerRef.current, chatOutput);
        // Load target server's chat from memory or disk
        let history = chatHistoryMap.current.get(id);
        if (!history || history.length === 0) {
            try {
                const result = await channelHistoryLoad(agentChatKey(id), 0, MA_PAGE_SIZE);
                if (result.messages.length > 0) {
                    history = result.messages.map(m => fromDisk(m));
                    chatHistoryMap.current.set(id, history);
                } else {
                    history = [];
                }
            } catch { history = []; }
        }
        setChatOutput(history);
        prevServerRef.current = id;
        setSelectedServerId(id);
    }, [chatOutput, fromDisk]);

    // Load models from config — refresh on mount and on window focus
    const loadModels = useCallback(() => {
        if (!api.getModels) return;
        api.getModels().then(loaded => {
            setModels(loaded);
            if (agentModel && loaded.length > 0 && !loaded.some(m => m.internalId === agentModel)) {
                setAgentModel(null);
            } else if (loaded.length === 0) {
                setAgentModel(null);
            }
        }).catch(e => console.error('Load models failed:', e));
    }, [agentModel]);

    useEffect(() => {
        loadModels();
        window.addEventListener('focus', loadModels);
        window.addEventListener('models-changed', loadModels);
        return () => {
            window.removeEventListener('focus', loadModels);
            window.removeEventListener('models-changed', loadModels);
        };
    }, [loadModels]);

    const agentModelData = models.find(m => m.internalId === agentModel);

    // Notify parent about running state
    useEffect(() => {
        onAgentRunningChange?.(!!agentModel);
    }, [agentModel, onAgentRunningChange]);

    // Auto-scroll
    useEffect(() => { logsEndRef.current?.scrollIntoView({ behavior: 'smooth' }); }, [appLogs]);
    useEffect(() => { chatEndRef.current?.scrollIntoView({ behavior: 'smooth' }); }, [chatOutput]);

    // Subscribe to agent events
    useEffect(() => {
        let unlisten: (() => void) | null = null;
        let cancelled = false;
        api.listenAgentEvents((event: AgentEvent) => {
            if (cancelled) return;
            switch (event.type) {
                case 'text_delta':
                    setChatOutput(prev => {
                        const last = prev[prev.length - 1];
                        if (last && last.type === 'assistant') {
                            return [...prev.slice(0, -1), { ...last, text: last.text + event.text }];
                        }
                        // First chunk of a new assistant message — notify parent
                        onNewMessage?.();
                        return [...prev, { type: 'assistant', text: event.text }];
                    });
                    break;
                case 'thinking':
                    // Thinking text is handled by TerminalStatusBar via agentState,
                    // not stored in chatOutput (it's not user-visible history)
                    break;
                case 'tool_call_start':
                case 'tool_call_args':
                    // Tool call progress is shown in TerminalStatusBar only,
                    // not stored in chatOutput
                    break;
                case 'tool_result':
                    // Tool results are internal agent data, not user-visible chat
                    break;
                case 'done':
                    setIsProcessing(false);
                    setAgentState('idle');
                    break;
                case 'error': {
                    const key = errorToKey(event.message);
                    const type = key === 'error.userCancelled' ? 'cancelled' : 'error';
                    setChatOutput(prev => [...prev, { type, text: '', i18nKey: key }]);
                    setIsProcessing(false);
                    setAgentState('idle');
                    break;
                }
                case 'state':
                    setAgentState(event.state);
                    break;
            }
        }).then(fn => {
            if (cancelled) {
                fn(); // Already unmounted, clean up immediately
            } else {
                unlisten = fn;
            }
        });
        return () => {
            cancelled = true;
            unlisten?.();
        };
    }, []);

    // Send logs to AI
    const handleSendLogsToAI = useCallback(async () => {
        if (isProcessing) return;
        const errorLogs = appLogs.filter(l => l.category === 'ERROR').slice(-10);
        const recentLogs = appLogs.slice(-20);
        const logsToSend = errorLogs.length > 0 ? errorLogs : recentLogs;
        const logsText = logsToSend.map(l => `[${l.timestamp}] [${l.category}] ${l.message}`).join('\n');
        const userMsg = chatInput.trim();
        const prompt = userMsg
            ? `Analyze these system logs and provide suggestions:\n\n${logsText}\n\nUser note: ${userMsg}`
            : `Analyze these system logs and provide suggestions:\n\n${logsText}`;

        setChatInput('');
        // Delegate to handleChatSend logic by setting chatInput and calling
        handleChatSendInternal(prompt);
    }, [agentModel, chatInput, appLogs, isProcessing]);

    // Internal send function
    const handleChatSendInternal = useCallback(async (message: string, displayText?: string, chips?: BubbleChip[]) => {
        if (isProcessing || !message.trim()) return;
        setIsProcessing(true);
        // Use display text + chips if provided (chip-send path), else full message text
        setChatOutput(prev => [...prev, { type: 'user', text: (displayText ?? message).trim(), chips }]);
        const modelData = models.find(m => m.internalId === agentModel);
        if (!modelData) {
            setChatOutput(prev => [...prev, { type: 'error', text: '', i18nKey: 'error.noModelSelected' }]);
            setIsProcessing(false);
            return;
        }

        try {
            // Triple-fallback protocol strategy:
            //   1. Has anthropicUrl  → use it directly as Anthropic
            //   2. Only baseUrl      → derive Anthropic URL (/v1 → /anthropic) and try first
            //   3. Backend gets 400  → auto-downgrade to OpenAI base_url
            const deriveAnthropicUrl = (base: string): string | null => {
                if (!base) return null;
                // Replace trailing /v1 or /v1/ with /anthropic (works for local LLM proxy)
                const stripped = base.trim().replace(/\/v1\/?$/, '');
                // Only derive if the original URL had /v1 (to avoid random derivations)
                if (stripped !== base.trim()) return `${stripped}/anthropic`;
                return null;
            };

            const anthropicUrl = modelData.anthropicUrl || deriveAnthropicUrl(modelData.baseUrl || '');
            await api.sendAgentMessage({
                message: message.trim(),
                model_id: modelData.internalId,
                // Always pass OpenAI URL as base_url (OpenAI fallback)
                base_url: modelData.baseUrl || '',
                api_key: modelData.apiKey,
                model_name: modelData.modelId || modelData.name,
                // Start with Anthropic when available; backend downgrades to OpenAI on 400
                provider: anthropicUrl ? 'anthropic' : 'openai',
                anthropic_url: anthropicUrl || undefined,
                proxy_url: modelData.proxyUrl,
                server_ids: selectedServerId === 'local' ? [] : [selectedServerId],
                skills: [],
                locale: locale || undefined,
            });
        } catch (e) {
            const key = errorToKey(String(e));
            const type = key === 'error.userCancelled' ? 'cancelled' : 'error';
            setChatOutput(prev => [...prev, { type, text: '', i18nKey: key }]);
            setIsProcessing(false);
        }
    }, [agentModel, models, isProcessing, selectedServerId, locale]);


    // Chat send (from input)
    const handleChatSend = useCallback(async () => {
        if (!chatInput.trim()) return;
        const msg = chatInput.trim();
        setChatInput('');
        handleChatSendInternal(msg);
    }, [chatInput, handleChatSendInternal]);

    return (
        <MotherAgentContext.Provider value={{
            appLogs, models, detectedTools, onClearLogs,
            agentModel, setAgentModel,
            chatInput, setChatInput,
            chatOutput, isProcessing, agentModelData, agentState,
            chatInputFocused, setChatInputFocused,
            chatCursorPos, setChatCursorPos,
            chatInputRef, chatEndRef, logsEndRef,
            handleSendLogsToAI, handleChatSend,
            sendMessage: handleChatSendInternal,

            sshServers, addSSHServer, removeSSHServer,
            selectedServerId, selectServer,
            clearChat: () => {
                setChatOutput([]);
                persistence.clearHistory();
                api.resetAgent(selectedServerId).catch(() => { });
            },
            abortAgent: () => {
                api.abortAgent(selectedServerId).catch(() => { });
                // Frontend safety net: force reset after 3s if backend doesn't respond
                setTimeout(() => {
                    setIsProcessing(prev => {
                        if (prev) {
                            setChatOutput(o => [...o, { type: 'cancelled', text: '', i18nKey: 'error.userCancelled' }]);
                            setAgentState('idle');
                        }
                        return false;
                    });
                }, 3000);
            },
            maDiskTotal: persistence.diskTotal,
            loadOlderChat: async () => {
                const count = await persistence.loadOlderChat();
                return count > 0 ? chatOutput.slice(0, count) : [];
            },
        }}>
            {children}
        </MotherAgentContext.Provider>
    );
}

// ===== Main Content (center area) �?CHAT =====
export function MotherAgentMain() {
    const { t, locale } = useI18n();
    const {
        models,
        agentModel, setAgentModel,
        chatInput, setChatInput,
        chatOutput, isProcessing, agentModelData, agentState,
        chatEndRef,
        handleChatSend,
        sendMessage,
        detectedTools,

        sshServers, selectedServerId,
        clearChat, abortAgent,
        maDiskTotal, loadOlderChat,
    } = useMotherAgent();
    const [publicIP, setPublicIP] = useState('...');
    const [remoteHints, setRemoteHints] = useState<Array<{ action: string; agent?: string }>>([]);
    const [serverModel, setServerModel] = useState<string | null>(null);
    const chatInputRef = useRef<HTMLTextAreaElement>(null!);
    const fileInputRef = useRef<HTMLInputElement>(null!);
    const imageInputRef = useRef<HTMLInputElement>(null!);

    // Model picker state
    const [showModelPicker, setShowModelPicker] = useState(false);
    const modelPickerRef = useRef<HTMLDivElement>(null!);
    const [pendingModels, setPendingModels] = useState<Array<{ id: string; name: string; modelId?: string }>>([]);
    const [pendingFiles, setPendingFiles] = useState<Array<{ id: string; name: string; type: 'file' | 'image'; preview?: string }>>([]);

    // Wrap handleChatSend to append pending model/file info as text
    const localSend = useCallback(() => {
        const hasModels = pendingModels.length > 0;
        const hasFiles = pendingFiles.length > 0;

        if (hasModels || hasFiles) {
            const mdList = pendingModels.map(pm => {
                const md = models.find(m => m.internalId === pm.id);
                return {
                    id: pm.id, name: pm.name, modelId: pm.modelId,
                    baseUrl: md?.baseUrl, anthropicUrl: md?.anthropicUrl,
                    apiKey: md?.apiKey, proxyUrl: md?.proxyUrl,
                };
            });
            const { messageText, chips } = buildPendingMessage(
                chatInput,
                pendingFiles,
                mdList,
                [],
            );

            setPendingModels([]);
            setPendingFiles([]);
            setChatInput('');
            sendMessage(messageText, chatInput.trim(), chips);
        } else {
            handleChatSend();
        }
    }, [pendingModels, pendingFiles, models, chatInput, setChatInput, handleChatSend, sendMessage]);



    // Close model picker on outside click
    useEffect(() => {
        if (!showModelPicker) return;
        const handler = (e: MouseEvent) => {
            if (modelPickerRef.current && !modelPickerRef.current.contains(e.target as Node)) {
                setShowModelPicker(false);
            }
        };
        document.addEventListener('mousedown', handler);
        return () => document.removeEventListener('mousedown', handler);
    }, [showModelPicker]);

    // File handling
    const handleFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
        const files = e.target.files;
        if (!files) return;
        Array.from(files).forEach(f => {
            const id = `file-${Date.now()}-${f.name}`;
            setPendingFiles(prev => [...prev, { id, name: f.name, type: 'file' }]);
        });
        e.target.value = '';
    };

    const handleImageSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
        const files = e.target.files;
        if (!files) return;
        Array.from(files).forEach(f => {
            const id = `img-${Date.now()}-${f.name}`;
            const reader = new FileReader();
            reader.onload = () => {
                setPendingFiles(prev => [...prev, { id, name: f.name, type: 'image', preview: reader.result as string }]);
            };
            reader.readAsDataURL(f);
        });
        e.target.value = '';
    };

    useEffect(() => {
        fetch('https://api.ipify.org?format=text')
            .then(r => r.text())
            .then(ip => setPublicIP(ip))
            .catch(() => setPublicIP('offline'));
        // Load quick-hint buttons from remote config
        fetch('https://echobird.ai/api/mother/hints.json')
            .then(r => r.json())
            .then(data => {
                // Filter out deprecated deploy-LLM action; keep deployBridge (core Bridge CLI feature)
                const hints = (data.hints || []).filter((h: any) => h.action !== 'deployLlm');
                // Always include local CLI Bridge install hint
                setRemoteHints([{ action: 'install', agent: 'Echobird CLI Bridge' }, ...hints]);
            })
            .catch(() => {
                // Offline: still show CLI Bridge hint
                setRemoteHints([{ action: 'install', agent: 'Echobird CLI Bridge' }]);
            });
    }, []);

    // Poll Local Server status
    useEffect(() => {
        const check = async () => {
            try {
                const info = await api.getLlmServerInfo();
                setServerModel(info.running ? (info.modelName || 'unknown') : null);
            } catch { setServerModel(null); }
        };
        check();
        const interval = setInterval(check, 3000);
        return () => clearInterval(interval);
    }, []);

    // Scroll management
    const chatContainerRef = useRef<HTMLDivElement>(null!);
    const autoFollowRef = useRef(true);
    const [showScrollBtn, setShowScrollBtn] = useState(false);
    const PAGE_SIZE = MA_PAGE_SIZE;
    const [displayCount, setDisplayCount] = useState(PAGE_SIZE);
    const [showSkeleton, setShowSkeleton] = useState(false);

    // Reset pagination when server changes
    useEffect(() => { setDisplayCount(PAGE_SIZE); }, [selectedServerId]);

    const handleScroll = () => {
        const container = chatContainerRef.current;
        if (!container) return;
        const isAtBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
        autoFollowRef.current = isAtBottom;
        setShowScrollBtn(!isAtBottom && chatOutput.length > 0);

        if (container.scrollTop !== 0) return;

        // Phase 1: more in-memory messages to show
        if (displayCount < chatOutput.length) {
            setShowSkeleton(true);
            const prevScrollHeight = container.scrollHeight;
            setTimeout(() => {
                setShowSkeleton(false);
                setDisplayCount(c => Math.min(c + PAGE_SIZE, chatOutput.length));
                requestAnimationFrame(() => {
                    if (chatContainerRef.current) {
                        chatContainerRef.current.scrollTop = chatContainerRef.current.scrollHeight - prevScrollHeight;
                    }
                });
            }, 300);
            return;
        }

        // Phase 2: load older batch from disk when in-memory is exhausted
        const alreadyLoaded = chatOutput.length;
        if (alreadyLoaded >= maDiskTotal) return;

        setShowSkeleton(true);
        const prevScrollHeight2 = container.scrollHeight;
        loadOlderChat().then(older => {
            setShowSkeleton(false);
            if (older.length === 0) return;
            setDisplayCount(c => c + older.length);
            requestAnimationFrame(() => {
                if (chatContainerRef.current) {
                    chatContainerRef.current.scrollTop = chatContainerRef.current.scrollHeight - prevScrollHeight2;
                }
            });
        }).catch(() => { setShowSkeleton(false); });
    };

    useEffect(() => {
        if (autoFollowRef.current && chatEndRef.current) {
            chatEndRef.current.scrollIntoView({ behavior: 'auto' });
        }
    }, [chatOutput]);

    const scrollToBottom = () => {
        autoFollowRef.current = true;
        setShowScrollBtn(false);
        chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    };

    return (
        <div className="flex flex-col h-full">
            {/* Chat conversation area */}
            <div className="relative flex-1">
                <div ref={chatContainerRef} onScroll={handleScroll} className="absolute inset-0 overflow-y-auto slim-scroll p-4">
                    {/* Quick prompt hints — scrolls with content */}
                    <div className="mb-2 select-none">
                        {remoteHints.length > 0 && (
                            <div className="flex flex-wrap gap-2 py-2">
                                {remoteHints.map((hint, i) => {
                                    const i18nKey = `mother.hint${hint.action[0].toUpperCase()}${hint.action.slice(1)}` as any;
                                    const label = t(i18nKey).replace('{agent}', hint.agent || '');
                                    // Skip hints whose i18n key was removed (label equals raw key)
                                    if (label === i18nKey) return null;
                                    return (
                                        <button
                                            key={i}
                                            onClick={() => { setChatInput(label); chatInputRef.current?.focus(); }}
                                            className="px-3 py-1 text-xs rounded-full border border-cyber-accent-secondary/20 text-cyber-accent-secondary/70 hover:bg-cyber-accent-secondary/10 hover:text-cyber-accent-secondary transition-all cursor-pointer"
                                        >
                                            {label}
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                    </div>

                    {/* Chat messages — bubble UI */}
                        <div className="pt-2 pb-2">
                            {/* Skeleton placeholders — shown briefly when lazy-loading older messages */}
                            {showSkeleton && [0, 1, 2].map(i => (
                                <ChatBubble key={`sk-${i}`} role="skeleton" content="" variant="mother" />
                            ))}
                            {chatOutput.slice(-displayCount).map((msg, i, arr) => {
                                if (msg.type === 'user') {
                                    return <ChatBubble key={i} role="user" content={msg.text} variant="mother" chips={msg.chips} />;
                                }
                                if (msg.type === 'assistant') {
                                    const retryMatch = msg.text.match(/__CONN_RETRY__:(\d+)\/(\d+)/);
                                    if (retryMatch) {
                                        const label = t('mother.connectionRetrying').replace('{n}', retryMatch[1]).replace('{total}', retryMatch[2]);
                                        return <ChatBubble key={i} role="retry" content={label} variant="mother" />;
                                    }
                                    const isLast = arr.slice(i + 1).every(m => m.type !== 'assistant');
                                    const lastOutput = chatOutput[chatOutput.length - 1];
                                    const isCurrentResponse = isLast && lastOutput?.type === 'assistant';
                                    return <ChatBubble key={i} role="assistant" content={msg.text} variant="mother" isStreaming={isProcessing && isCurrentResponse} />;
                                }
                                if (msg.type === 'cancelled') {
                                    const text = msg.i18nKey ? t(msg.i18nKey as import('../i18n/types').TKey) : msg.text;
                                    return <div key={i} className="flex justify-center my-4"><span className="text-cyber-text-muted/35 text-xs font-mono">{text}</span></div>;
                                }
                                if (msg.type === 'error') {
                                    const text = msg.i18nKey ? t(msg.i18nKey as import('../i18n/types').TKey) : msg.text;
                                    const failedMatch = msg.text.match(/__CONN_FAILED__:(\d+)/);
                                    if (failedMatch) {
                                        return <ChatBubble key={i} role="error"
                                            content={t('mother.connectionFailed').replace('{n}', failedMatch[1])}
                                            subContent={t('mother.connectionHint')}
                                            variant="mother" />;
                                    }
                                    return <ChatBubble key={i} role="error" content={text} variant="mother" />;
                                }
                                return null;
                            })}
                            {/* Typing indicator — show when processing and no new assistant response has started */}
                            {isProcessing && (chatOutput.length === 0 || chatOutput[chatOutput.length - 1]?.type !== 'assistant') && (
                                <ChatBubble role="assistant" content="" variant="mother" isStreaming={true} />
                            )}
                            <div ref={chatEndRef} />
                        </div>
                </div>
                {/* Scroll to bottom button */}
                {showScrollBtn && (
                    <button
                        onClick={scrollToBottom}
                        className="absolute bottom-3 right-3 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-accent-secondary hover:border-cyber-accent-secondary/50 transition-colors z-10"
                    >
                        <ChevronDown className="w-4 h-4" />
                    </button>
                )}
            </div>

            {/* Rich input area */}
            <div className="flex-shrink-0 mt-1 mb-1">
                <div className="bg-cyber-terminal rounded-lg relative">
                    {/* Pending attachments chips — shared component */}
                    <PendingChipsRow
                        files={pendingFiles}
                        onRemoveFile={id => setPendingFiles(prev => prev.filter(x => x.id !== id))}
                        models={pendingModels}
                        onRemoveModel={id => setPendingModels(prev => prev.filter(x => x.id !== id))}
                    />
                    <textarea
                        ref={chatInputRef}
                        value={chatInput}
                        onChange={e => setChatInput(e.target.value)}
                        onKeyDown={e => {
                            if (e.key === 'Enter' && !e.shiftKey) {
                                e.preventDefault();
                                if (!isProcessing) localSend();
                            }
                        }}
                        placeholder={t('mother.enterMessage')}
                        disabled={isProcessing}
                        rows={2}
                        className="w-full bg-transparent px-4 py-2 text-sm text-[#DED9D2] font-sans font-medium outline-none placeholder:text-[#DED9D2]/40 disabled:opacity-30 resize-none"
                    />
                    {/* Bottom toolbar */}
                    <div className="flex items-center justify-between px-3 py-1.5">
                        <div className="flex items-center gap-1 relative">
                            <button
                                onClick={() => fileInputRef.current?.click()}
                                disabled={isProcessing}
                                className="p-1 text-cyber-accent-secondary/40 hover:text-cyber-accent-secondary transition-colors disabled:opacity-20"
                            >
                                <Paperclip size={15} />
                            </button>
                            <button
                                onClick={() => imageInputRef.current?.click()}
                                disabled={isProcessing}
                                className="p-1 text-cyber-accent-secondary/40 hover:text-cyber-accent-secondary transition-colors disabled:opacity-20"
                            >
                                <ImageIcon size={15} />
                            </button>
                            <button
                                onClick={() => setShowModelPicker(prev => !prev)}
                                disabled={isProcessing || pendingModels.length >= 5}
                                className={`p-1 transition-colors disabled:opacity-20 ${showModelPicker ? 'text-cyber-accent-secondary' : 'text-cyber-accent-secondary/40 hover:text-cyber-accent-secondary'}`}
                            >
                                <KeyRound size={15} />
                            </button>
                            {/* Model picker popover */}
                            {showModelPicker && (
                                <div
                                    ref={modelPickerRef}
                                    className="absolute bottom-full left-0 mb-2 w-64 max-h-48 overflow-y-auto slim-scroll bg-cyber-bg border border-cyber-border/60 rounded-lg shadow-lg z-50 custom-scrollbar"
                                >
                                    {models.length === 0 ? (
                                        <div className="px-3 py-2 text-xs text-cyber-text-muted/50 font-mono">{t('mother.noModels')}</div>
                                    ) : (
                                        models.map(m => (
                                            <button
                                                key={m.internalId}
                                                onClick={() => {
                                                    if (!pendingModels.some(pm => pm.id === m.internalId) && pendingModels.length < 5) {
                                                        setPendingModels(prev => [...prev, { id: m.internalId, name: m.name, modelId: m.modelId }]);
                                                    }
                                                    setShowModelPicker(false);
                                                }}
                                                className="w-full text-left px-3 py-2 text-xs font-mono hover:bg-cyber-accent-secondary/10 transition-colors border-b border-cyber-border/10 last:border-b-0 flex items-center gap-2 text-cyber-text"
                                            >
                                                {(() => {
                                                    const icon = getModelIcon(m.name, m.modelId || '');
                                                    return icon ? (
                                                        <img src={icon} alt="" className="w-5 h-5 flex-shrink-0" />
                                                    ) : (
                                                        <KeyRound size={14} className="text-cyber-accent-secondary/40 flex-shrink-0" />
                                                    );
                                                })()}
                                                <div className="min-w-0">
                                                    <div className="font-bold truncate">{m.name}</div>
                                                    <div className="text-cyber-text-muted/50 truncate text-[10px]">{m.modelId || m.baseUrl}</div>
                                                </div>
                                            </button>
                                        ))
                                    )}
                                </div>
                            )}
                        </div>
                        <div className="flex items-center gap-2">
                            <button
                                onClick={() => { clearChat(); }}
                                disabled={isProcessing || chatOutput.length === 0}
                                className="p-1 text-cyber-accent-secondary/40 hover:text-cyber-accent-secondary transition-colors disabled:opacity-20"
                            >
                                <RotateCcw size={15} />
                            </button>
                            <span className="text-xs font-mono text-cyber-accent-secondary/80 truncate max-w-[160px]">
                                {(() => {
                                    if (selectedServerId === 'local') return t('mother.local');
                                    const server = sshServers.find(s => s.id === selectedServerId);
                                    return server ? server.host : t('mother.noServer');
                                })()}
                            </span>
                            {isProcessing ? (
                                <button
                                    onClick={() => abortAgent()}
                                    className="p-1 text-red-400/80 hover:text-red-400 transition-colors"
                                >
                                    <Square size={16} fill="currentColor" />
                                </button>
                            ) : (
                                <button
                                    onClick={localSend}
                                    disabled={!chatInput.trim()}
                                    className="w-6 h-6 rounded-lg flex items-center justify-center bg-cyber-accent-secondary hover:brightness-110 transition-all disabled:opacity-20"
                                >
                                    <Send size={15} className="text-cyber-bg" />
                                </button>
                            )}
                        </div>
                    </div>
                </div>
                {/* Hidden file inputs */}
                <input ref={fileInputRef} type="file" multiple className="hidden" onChange={handleFileSelect} />
                <input ref={imageInputRef} type="file" multiple accept="image/*" className="hidden" onChange={handleImageSelect} />
            </div>
        </div>
    );
}

// ===== Title Bar Model Selector =====
export function MotherAgentModelSelector() {
    const { models, agentModel, setAgentModel } = useMotherAgent();
    const { t } = useI18n();
    return (
        <MiniSelect
            value={agentModel || ''}
            onChange={(val) => setAgentModel(val || null)}
            options={[
                { id: '', label: t('mother.selectModel') },
                ...models.map(m => ({ id: m.internalId, label: m.name }))
            ]}
            className="ml-auto w-52 min-w-[180px]"
            accent="blue"
        />
    );
}

// ===== Right Panel (aside area) — SERVERS =====
export function MotherAgentPanel() {
    const { setChatInput, chatInputRef, sshServers, addSSHServer, removeSSHServer, selectedServerId, selectServer, isProcessing } = useMotherAgent();
    const confirm = useConfirm();
    const { t } = useI18n();

    const [panelTab, setPanelTab] = useState<'servers' | 'guide'>('servers');
    const [showSSHModal, setShowSSHModal] = useState(false);
    const [sshForm, setSSHForm] = useState({ host: '', port: '22', username: '', password: '', alias: '', showPassword: false });
    const [sshTestResult, setSSHTestResult] = useState<{ success: boolean; message: string } | null>(null);
    const [sshTesting, setSSHTesting] = useState(false);
    const [expandedGuide, setExpandedGuide] = useState<string | null>(null);

    const handleSSHTest = async () => {
        if (!sshForm.host.trim() || !sshForm.username.trim()) return;
        setSSHTesting(true);
        setSSHTestResult(null);
        try {
            const result = await api.sshTestConnection(
                sshForm.host.trim(),
                parseInt(sshForm.port) || 22,
                sshForm.username.trim(),
                sshForm.password,
            );
            setSSHTestResult(result);
        } catch (e) {
            setSSHTestResult({ success: false, message: String(e) });
        } finally {
            setSSHTesting(false);
        }
    };



    return (
        <>
            {/* Header with tabs */}
            <div className="p-2 flex items-center justify-between bg-transparent">
                <div className="flex gap-1">
                    <button
                        onClick={() => setPanelTab('servers')}
                        className={`px-3 py-1.5 text-xs font-bold rounded transition-colors ${panelTab === 'servers'
                            ? 'bg-cyber-accent-secondary text-black'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('mother.servers')}
                    </button>
                    <button
                        onClick={() => setPanelTab('guide')}
                        className={`px-3 py-1.5 text-xs font-bold rounded transition-colors ${panelTab === 'guide'
                            ? 'bg-cyber-accent-secondary/80 text-black'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('mother.sshGuide')}
                    </button>
                </div>
            </div>

            {/* SSH Add Modal */}
            {showSSHModal && (
                <div
                    className="fixed inset-0 z-[9998] flex items-center justify-center"
                    onKeyDown={e => { if (e.key === 'Escape') setShowSSHModal(false); }}
                >
                    {/* Backdrop */}
                    <div
                        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                        onClick={() => setShowSSHModal(false)}
                    />

                    <div
                        className="relative w-[400px] max-w-[90vw] border border-cyber-accent-secondary/30 bg-cyber-bg shadow-[0_0_30px_rgba(0,212,255,0.08)] rounded-xl overflow-hidden"
                        onClick={e => e.stopPropagation()}
                    >
                        {/* Top accent line */}
                        <div className="h-[2px] w-full bg-gradient-to-r from-cyber-accent-secondary/60 via-cyber-accent-secondary/40 to-transparent" />

                        {/* Header */}
                        <div className="px-5 pt-4 pb-3 flex items-center justify-between">
                            <div className="flex items-center gap-2">
                                <span className="text-cyber-accent-secondary font-mono text-xs opacity-60">&gt;_</span>
                                <span className="text-sm font-mono font-bold tracking-wider text-cyber-accent-secondary">{t('mother.addServer')}</span>
                            </div>
                            <button
                                onClick={() => setShowSSHModal(false)}
                                className="text-cyber-text-secondary hover:text-cyber-text transition-colors"
                            >
                                <X size={14} />
                            </button>
                        </div>

                        {/* Form */}
                        <div className="px-5 pb-5">
                            <div className="space-y-4">
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.hostIp')}</label>
                                    <input
                                        type="text"
                                        placeholder={t('mother.hostPlaceholder')}
                                        value={sshForm.host}
                                        onChange={e => setSSHForm(f => ({ ...f, host: e.target.value }))}
                                        className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent-secondary focus:outline-none rounded-button"
                                        autoFocus
                                    />
                                </div>
                                {/* Display Name — optional alias */}
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">
                                        {t('mother.displayName')} <span className="opacity-50">({t('mother.optional')})</span>
                                    </label>
                                    <input
                                        type="text"
                                        placeholder={t('mother.displayNamePlaceholder')}
                                        value={sshForm.alias}
                                        onChange={e => setSSHForm(f => ({ ...f, alias: e.target.value }))}
                                        className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent-secondary focus:outline-none rounded-button"
                                    />
                                </div>
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.port')}</label>
                                    <input
                                        type="number"
                                        placeholder="22"
                                        value={sshForm.port}
                                        onChange={e => setSSHForm(f => ({ ...f, port: e.target.value }))}
                                        className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent-secondary focus:outline-none rounded-button no-spinner"
                                    />
                                </div>
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.username')}</label>
                                    <input
                                        type="text"
                                        placeholder={t('mother.userPlaceholder')}
                                        value={sshForm.username}
                                        onChange={e => setSSHForm(f => ({ ...f, username: e.target.value }))}
                                        className="w-full bg-black border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-accent-secondary focus:outline-none rounded-button"
                                    />
                                </div>
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.passwordKey')}</label>
                                    <div className="relative">
                                        <input
                                            type="text"
                                            placeholder={t('mother.passwordPlaceholder')}
                                            value={sshForm.password.startsWith('enc:v1:') ? '•••••••••••••••' : sshForm.password}
                                            onChange={e => setSSHForm(f => ({ ...f, password: e.target.value }))}
                                            className="w-full bg-black border border-cyber-border px-2 py-1.5 pr-8 text-xs text-cyber-text font-mono focus:border-cyber-accent-secondary focus:outline-none rounded-button"
                                            readOnly={sshForm.password.startsWith('enc:v1:')}
                                        />
                                        <button
                                            type="button"
                                            disabled={!sshForm.password}
                                            onClick={async () => {
                                                if (!sshForm.password) return;
                                                if (sshForm.password.startsWith('enc:v1:')) {
                                                    // Decrypt
                                                    try {
                                                        const plain = await api.decryptSSHPassword(sshForm.password);
                                                        setSSHForm(f => ({ ...f, password: plain || '' }));
                                                    } catch {
                                                        setSSHForm(f => ({ ...f, password: '' }));
                                                    }
                                                } else {
                                                    // Encrypt
                                                    try {
                                                        const encrypted = await api.encryptSSHPassword(sshForm.password);
                                                        setSSHForm(f => ({ ...f, password: encrypted }));
                                                    } catch {
                                                        // stay plaintext on failure
                                                    }
                                                }
                                            }}
                                            className={`absolute right-2 top-1/2 -translate-y-1/2 transition-colors hover:opacity-80 ${!sshForm.password ? 'opacity-20' : ''}`}
                                        >
                                            {sshForm.password.startsWith('enc:v1:') ? (
                                                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-cyber-accent">
                                                    <rect x="3" y="11" width="18" height="11" rx="2" />
                                                    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                                                </svg>
                                            ) : (
                                                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-cyber-text-muted">
                                                    <rect x="3" y="11" width="18" height="11" rx="2" />
                                                    <path d="M7 11V7a5 5 0 0 1 9.9-1" />
                                                </svg>
                                            )}
                                        </button>
                                    </div>
                                    <div className="min-h-[36px] mt-1">
                                        {sshForm.password.startsWith('enc:v1:') && (
                                            <div className="text-xs leading-tight text-cyber-accent/60">
                                                {t('mother.encrypted')}
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </div>
                        </div>

                        {/* Test connection button */}
                        <div className="px-5 pb-4">
                            <button
                                onClick={handleSSHTest}
                                disabled={sshTesting || !sshForm.host.trim() || !sshForm.username.trim()}
                                className="w-full py-2 text-xs font-mono font-bold tracking-wider border border-cyber-accent-secondary/40 text-cyber-accent-secondary rounded-button hover:bg-cyber-accent-secondary/10 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                            >
                                {sshTesting ? t('mother.testing') : t('mother.testConnection')}
                            </button>
                            {sshTestResult && !sshTesting && (
                                <div className={`text-[11px] font-mono mt-2 px-3 py-2 rounded border ${sshTestResult.success
                                    ? 'border-green-500/30 text-green-400 bg-green-500/5'
                                    : 'border-red-500/30 text-red-400 bg-red-500/5'
                                    }`}>
                                    {sshTestResult.message}
                                </div>
                            )}
                        </div>

                        {/* Footer buttons */}
                        <div className="flex border-t border-cyber-border">
                            <button
                                onClick={() => { setShowSSHModal(false); setSSHTestResult(null); }}
                                className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-text-secondary hover:text-cyber-text hover:bg-white/5 transition-all border-r border-cyber-border"
                            >
                                {t('mother.cancel')}
                            </button>
                            <button
                                onClick={async () => {
                                    if (!sshForm.host.trim()) return;
                                    const host = sshForm.host.trim();
                                    const username = sshForm.username.trim();
                                    // Auto-overwrite: remove existing server with same host + username
                                    const existing = sshServers.find(s => s.host === host && s.username === username);
                                    if (existing) await removeSSHServer(existing.id);
                                    const newServer = {
                                        id: Date.now().toString(),
                                        host,
                                        port: sshForm.port || '22',
                                        username,
                                        password: sshForm.password,
                                        alias: sshForm.alias.trim() || undefined,
                                    };
                                    await addSSHServer(newServer);
                                    setSSHForm({ host: '', port: '22', username: '', password: '', alias: '', showPassword: false });
                                    setSSHTestResult(null);
                                    setShowSSHModal(false);
                                }}
                                className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-cyber-accent-secondary hover:bg-cyber-accent-secondary/10 transition-all"
                            >
                                {t('mother.addServerBtn')}
                            </button>
                        </div>
                    </div>
                </div >
            )
            }

            {/* Content */}
            <div className="flex-1 p-2 overflow-y-auto slim-scroll">
                {panelTab === 'servers' ? (
                    /* ── SERVERS tab ── */
                    <div className="space-y-2">
                        {/* Server list */}
                        {/* Local server — always first */}
                        <div
                            onClick={() => !isProcessing && selectServer('local')}
                            className={`p-3 border rounded transition-all select-none flex items-center ${isProcessing && selectedServerId !== 'local' ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'} ${selectedServerId === 'local'
                                ? 'border-cyber-accent-secondary bg-cyber-accent-secondary/5 shadow-[0_0_10px_rgba(0,212,255,0.15)]'
                                : 'border-cyber-border hover:border-cyber-accent-secondary/50'
                                }`}
                        >
                            <div className="mr-3">
                                <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center transition-all ${selectedServerId === 'local' ? 'border-cyber-accent-secondary' : 'border-cyber-text-muted/30'}`}>
                                    {selectedServerId === 'local' && <div className="w-2 h-2 rounded-full bg-cyber-accent-secondary" />}
                                </div>
                            </div>
                            <div className="flex-1 min-w-0">
                                <div className="text-xs text-cyber-text-secondary mb-0.5 tracking-widest uppercase font-mono">{t('mother.local')}</div>
                                <div className="text-sm font-bold truncate text-cyber-accent-secondary font-mono">127.0.0.1</div>
                            </div>
                        </div>
                        {/* SSH servers */}
                        {sshServers.map(server => (
                            <div
                                key={server.id}
                                onClick={() => !isProcessing && selectServer(server.id)}
                                className={`p-3 border rounded transition-all select-none flex items-center ${isProcessing && selectedServerId !== server.id ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'} ${selectedServerId === server.id
                                    ? 'border-cyber-accent-secondary bg-cyber-accent-secondary/5 shadow-[0_0_10px_rgba(0,212,255,0.15)]'
                                    : 'border-cyber-border hover:border-cyber-accent-secondary/50'
                                    }`}
                            >
                                <div className="mr-3">
                                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center transition-all ${selectedServerId === server.id ? 'border-cyber-accent-secondary' : 'border-cyber-text-muted/30'}`}>
                                        {selectedServerId === server.id && <div className="w-2 h-2 rounded-full bg-cyber-accent-secondary" />}
                                    </div>
                                </div>
                                <div className="flex-1 min-w-0">
                                    <div className="flex items-center gap-1 mb-0.5">
                                        <span className="text-xs text-cyber-text-secondary tracking-widest uppercase font-mono truncate flex-1 min-w-0">{server.alias || t('mother.local')}</span>
                                        <button
                                            onClick={async (e) => {
                                                e.stopPropagation();
                                                // Load encrypted password from backend
                                                let savedPassword = '';
                                                try {
                                                    const servers = await api.loadSSHServers();
                                                    const saved = servers.find(s => s.id === server.id);
                                                    if (saved?.password) savedPassword = saved.password;
                                                } catch { }
                                                setSSHForm({
                                                    host: server.host,
                                                    port: server.port,
                                                    username: server.username,
                                                    password: savedPassword,
                                                    alias: server.alias || '',
                                                    showPassword: false,
                                                });
                                                setSSHTestResult(null);
                                                setShowSSHModal(true);
                                            }}
                                            className="text-xs font-mono text-cyber-text-muted/50 hover:text-cyber-accent-secondary transition-colors flex-shrink-0"
                                        >
                                            [{t('btn.edit')}]
                                        </button>
                                        <button
                                            onClick={async (e) => {
                                                e.stopPropagation();
                                                const ok = await confirm({
                                                    title: t('mother.deleteServerTitle'),
                                                    message: t('mother.deleteServerMsg'),
                                                    confirmText: t('btn.delete'),
                                                    cancelText: t('btn.cancel'),
                                                    type: 'danger'
                                                });
                                                if (ok) removeSSHServer(server.id);
                                            }}
                                            className="text-xs font-mono text-cyber-text-muted/50 hover:text-red-500 transition-colors flex-shrink-0"
                                        >
                                            [{t('btn.delete')}]
                                        </button>
                                    </div>
                                    <div className="text-sm font-bold truncate text-cyber-accent-secondary font-mono">{server.username ? `${server.username}@` : ''}{server.host}{server.port !== '22' ? `:${server.port}` : ''}</div>
                                </div>
                            </div>
                        ))}
                        {/* Add button card */}
                        <button
                            onClick={() => {
                                setSSHForm({ host: '', port: '22', username: '', password: '', alias: '', showPassword: false });
                                setSSHTestResult(null);
                                setShowSSHModal(true);
                            }}
                            className="w-full p-4 border border-dashed border-cyber-accent-secondary/30 rounded hover:border-cyber-accent-secondary/60 hover:bg-cyber-accent-secondary/5 transition-all text-cyber-accent-secondary/60 hover:text-cyber-accent-secondary text-xs font-bold"
                        >
                            + {t('mother.addServer')}
                        </button>
                    </div>
                ) : (
                    /* ── SSH GUIDE tab (accordion) ── */
                    <div className="space-y-1 text-xs font-mono">
                        {[
                            {
                                id: 'cloud', label: 'Cloud Server', content: (
                                    <>
                                        <p className="text-cyber-text-muted/80">{t('ssh.cloudDesc')}</p>
                                        <div className="mt-2 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.usernameHint')}</span> — {t('ssh.cloudUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.passwordHint')}</span> — {t('ssh.cloudPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.ipHint')}</span> — {t('ssh.cloudIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'windows', label: 'Windows', content: (
                                    <>
                                        <ol className="space-y-0.5 text-cyber-text-muted/70 list-decimal list-inside">
                                            <li>{t('ssh.winStep1')}</li>
                                            <li>{t('ssh.winStep2')}</li>
                                            <li>{t('ssh.winStep3')}</li>
                                        </ol>
                                        <p className="text-cyber-accent-secondary pl-4 mt-0.5">Start-Service sshd</p>
                                        <p className="text-cyber-accent-secondary pl-4">Set-Service sshd -StartupType Automatic</p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.usernameHint')}</span> — {t('ssh.winUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.passwordHint')}</span> — {t('ssh.winPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.ipHint')}</span> — {t('ssh.winIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'macos', label: 'macOS', content: (
                                    <>
                                        <p className="text-cyber-text-muted/70">{t('ssh.macStep')}</p>
                                        <p className="text-cyber-text-muted/50 mt-0.5">{t('ssh.macOr')} <span className="text-cyber-accent-secondary">sudo systemsetup -setremotelogin on</span></p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.usernameHint')}</span> — {t('ssh.macUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.passwordHint')}</span> — {t('ssh.macPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.ipHint')}</span> — {t('ssh.macIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'linux', label: 'Linux', content: (
                                    <>
                                        <p className="text-cyber-accent-secondary">sudo apt install openssh-server</p>
                                        <p className="text-cyber-accent-secondary">sudo systemctl enable --now ssh</p>
                                        <p className="text-cyber-text-muted/50 text-xs">{t('ssh.linuxNote')}</p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.usernameHint')}</span> — {t('ssh.linuxUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.passwordHint')}</span> — {t('ssh.linuxPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.ipHint')}</span> — {t('ssh.linuxIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'android', label: 'Android (Termux)', content: (
                                    <>
                                        <p className="text-cyber-accent-secondary">pkg install openssh && sshd</p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.usernameHint')}</span> — {t('ssh.termuxUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.passwordHint')}</span> — {t('ssh.termuxPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.ipHint')}</span> — {t('ssh.termuxIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.portHint')}</span> — 8022</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'ios', label: 'iOS (iSH)', content: (
                                    <>
                                        <p className="text-cyber-accent-secondary">apk add openssh</p>
                                        <p className="text-cyber-accent-secondary">ssh-keygen -A && /usr/sbin/sshd</p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.usernameHint')}</span> — {t('ssh.ishUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.passwordHint')}</span> — {t('ssh.ishPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.ipHint')}</span> — {t('ssh.ishIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-accent-secondary">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                        ].map(section => (
                            <div key={section.id} className="border border-cyber-accent-secondary/20 rounded overflow-hidden">
                                <button
                                    onClick={() => setExpandedGuide(prev => prev === section.id ? null : section.id)}
                                    className="w-full px-3 py-2 flex items-center justify-between bg-cyber-accent-secondary/5 hover:bg-cyber-accent-secondary/10 transition-colors"
                                >
                                    <span className="text-cyber-accent-secondary font-bold text-sm">{section.label}</span>
                                    <ChevronDown size={14} className={`text-cyber-accent-secondary/60 transition-transform ${expandedGuide === section.id ? 'rotate-180' : ''}`} />
                                </button>
                                {expandedGuide === section.id && (
                                    <div className="px-3 py-2 space-y-1">
                                        {section.content}
                                    </div>
                                )}
                            </div>
                        ))}
                    </div>
                )}
            </div>
        </>
    );
}

