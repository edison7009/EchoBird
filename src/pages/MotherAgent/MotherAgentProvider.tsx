import React, { useState, useEffect, useRef, useCallback } from 'react';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { ModelConfig, AgentEvent } from '../../api/types';
import { useChatPersistence } from '../../hooks/useChatPersistence';
import type { DiskMsg } from '../../hooks/useChatPersistence';
import { errorToKey } from '../../utils/normalizeError';
import type { BubbleChip } from '../../components/chat/ChatBubble';
import type { ChatMessage } from './types';
import { MA_PAGE_SIZE } from './types';
import { MotherAgentContext } from './context';
import { useAppLogsStore } from '../../stores/appLogsStore';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';

// ===== Provider =====

export function MotherAgentProvider({ children }: { children: React.ReactNode }) {
    // From stores (replaces drilled props)
    const { appLogs, clearLogs: onClearLogs } = useAppLogsStore();
    const { detectedTools } = useToolsStore();
    const { motherPrefill: initialMessage, activePage, setMotherNewMessage } = useNavigationStore();
    const onNewMessage = useCallback(() => {
        if (activePage !== 'mother') setMotherNewMessage(true);
    }, [activePage, setMotherNewMessage]);
    const onAgentRunningChange = useNavigationStore(s => s.setAgentRunning);
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
    const abortTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
        useNavigationStore.getState().bumpSshServersVersion();
    }, []);
    const removeSSHServer = useCallback(async (id: string) => {
        setSSHServers(prev => prev.filter(s => s.id !== id));
        setSelectedServerId(prev => prev === id ? 'local' : prev);
        // Remove from backend
        await api.removeSSHServerFromDisk(id).catch(() => { });
        useNavigationStore.getState().bumpSshServersVersion();
    }, []);

    // Server selection (single-select)
    const [selectedServerId, setSelectedServerId] = useState('local');

    const prevServerRef = useRef('local');
    const agentChatKey = (id: string) => `agent_${id}`;

    // Shared mapper: ChatMessage → disk format
    const toDisk = useCallback((m: ChatMessage): DiskMsg | null => {
        if (m.type === 'user') return { role: 'user', content: m.text };
        if (m.type === 'assistant') {
            // Skip transient status messages — they're not real chat history
            if (m.text.includes('__CONN_RETRY__') || m.text.includes('__CONN_FAILED__')) return null;
            return { role: 'assistant', content: m.text };
        }
        if (m.type === 'error') return { role: 'system', content: (m as any).i18nKey || m.text };
        if (m.type === 'cancelled') return { role: 'system', content: (m as any).i18nKey || m.text };
        return null;
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
        // Load target server's chat from memory or localStorage
        let history = chatHistoryMap.current.get(id);
        if (!history || history.length === 0) {
            try {
                const raw = localStorage.getItem(`echobird:chat:${agentChatKey(id)}`);
                if (raw) {
                    const stored = JSON.parse(raw) as DiskMsg[];
                    if (Array.isArray(stored) && stored.length > 0) {
                        history = stored.map(m => fromDisk(m));
                        chatHistoryMap.current.set(id, history);
                    } else {
                        history = [];
                    }
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
                    // Reasoning is private — not surfaced into the chat stream
                    break;
                case 'tool_call_start':
                    setChatOutput(prev => [...prev, {
                        type: 'tool_call',
                        id: event.id,
                        name: event.name,
                        args: '',
                        status: 'running',
                    }]);
                    break;
                case 'tool_call_args':
                    setChatOutput(prev => prev.map(m =>
                        m.type === 'tool_call' && m.id === event.id
                            ? { ...m, args: m.args + event.args }
                            : m
                    ));
                    break;
                case 'tool_result':
                    setChatOutput(prev => prev.map(m =>
                        m.type === 'tool_call' && m.id === event.id
                            ? { ...m, status: event.success ? 'done' : 'failed', output: event.output }
                            : m
                    ));
                    break;
                case 'done':
                    if (abortTimeoutRef.current) { clearTimeout(abortTimeoutRef.current); abortTimeoutRef.current = null; }
                    setIsProcessing(false);
                    setAgentState('idle');
                    break;
                case 'error': {
                    if (abortTimeoutRef.current) { clearTimeout(abortTimeoutRef.current); abortTimeoutRef.current = null; }
                    const key = errorToKey(event.message);
                    // Skip duplicate cancelled message — already added immediately in abortAgent()
                    if (key !== 'error.userCancelled') {
                        setChatOutput(prev => [...prev, { type: 'error', text: '', i18nKey: key }]);
                    }
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
        // Clear any pending abort timeout from previous request
        if (abortTimeoutRef.current) { clearTimeout(abortTimeoutRef.current); abortTimeoutRef.current = null; }
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
            models,
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
                // Immediately show cancelled hint
                setChatOutput(o => [...o, { type: 'cancelled', text: '', i18nKey: 'error.userCancelled' }]);
                // Frontend safety net: force reset after 3s if backend doesn't respond
                if (abortTimeoutRef.current) clearTimeout(abortTimeoutRef.current);
                abortTimeoutRef.current = setTimeout(() => {
                    abortTimeoutRef.current = null;
                    setIsProcessing(prev => {
                        if (prev) {
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
