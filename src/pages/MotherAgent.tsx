import React, { useState, useEffect, useRef, useCallback, createContext, useContext } from 'react';
import { Paperclip, ImageIcon, KeyRound, Send, X, ChevronDown, Zap, Square, Lock, Trash2 } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { MiniSelect } from '../components/MiniSelect';
import { getModelIcon } from '../components/cards/ModelCard';
import { useI18n } from '../hooks/useI18n';
import { useConfirm } from '../components/ConfirmDialog';
import * as api from '../api/tauri';
import type { ModelConfig, LocalTool, AppLogEntry, AgentEvent } from '../api/types';

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
    | { type: 'user'; text: string }
    | { type: 'assistant'; text: string }
    | { type: 'thinking'; text: string }
    | { type: 'tool_call'; id: string; name: string; args: string; status: 'running' | 'done' }
    | { type: 'tool_result'; id: string; output: string; success: boolean }
    | { type: 'error'; text: string }
    | { type: 'state'; state: string };

// ===== Context (shared state between Main & Panel) =====
interface PendingSkill {
    id: string;
    name: string;
    github: string;
}

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
    sendMessage: (msg: string) => void;
    // pending skills (attached to next message)
    pendingSkills: PendingSkill[];
    addPendingSkill: (skill: PendingSkill) => void;
    removePendingSkill: (id: string) => void;
    // ssh servers
    sshServers: Array<{ id: string; host: string; port: string; username: string; alias?: string }>;
    addSSHServer: (server: { id: string; host: string; port: string; username: string; password: string; alias?: string }) => void;
    removeSSHServer: (id: string) => void;
    selectedServerId: string;
    selectServer: (id: string) => void;
    clearChat: () => void;
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
    children: React.ReactNode;
}

export function MotherAgentProvider({ appLogs, detectedTools, onClearLogs, onAgentRunningChange, onNewMessage, children }: MotherAgentProviderProps) {
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
    const [pendingSkills, setPendingSkills] = useState<PendingSkill[]>([]);

    const addPendingSkill = useCallback((skill: PendingSkill) => {
        setPendingSkills(prev => {
            if (prev.some(s => s.id === skill.id)) return prev;
            return [...prev, skill];
        });
    }, []);

    const removePendingSkill = useCallback((id: string) => {
        setPendingSkills(prev => prev.filter(s => s.id !== id));
    }, []);

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
    const prevServerRef = useRef('local');
    const selectServer = useCallback(async (id: string) => {
        // Save current chat to history map
        chatHistoryMap.current.set(prevServerRef.current, chatOutput);
        // Load target server's chat history (from memory or disk)
        let history = chatHistoryMap.current.get(id);
        if (!history || history.length === 0) {
            try {
                const diskHistory = await api.loadAgentHistory(id);
                history = diskHistory.map(h => ({
                    type: h.role === 'user' ? 'user' as const : 'assistant' as const,
                    text: h.text,
                }));
                chatHistoryMap.current.set(id, history);
            } catch { history = []; }
        }
        setChatOutput(history);
        prevServerRef.current = id;
        setSelectedServerId(id);
    }, [chatOutput]);

    // Load chat history from disk on mount
    useEffect(() => {
        api.loadAgentHistory('local').then(diskHistory => {
            if (diskHistory.length > 0) {
                const loaded = diskHistory.map(h => ({
                    type: h.role === 'user' ? 'user' as const : 'assistant' as const,
                    text: h.text,
                }));
                chatHistoryMap.current.set('local', loaded);
                setChatOutput(loaded);
            }
        }).catch(() => { });
    }, []);

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
                    setChatOutput(prev => [...prev, { type: 'thinking', text: event.text }]);
                    break;
                case 'tool_call_start':
                    setChatOutput(prev => [...prev,
                    { type: 'tool_call', id: event.id, name: event.name, args: '', status: 'running' as const }
                    ]);
                    break;
                case 'tool_call_args':
                    setChatOutput(prev => prev.map(m =>
                        m.type === 'tool_call' && m.id === event.id
                            ? { ...m, args: m.args + event.args }
                            : m
                    ));
                    break;
                case 'tool_result':
                    // Mark tool_call as done and add result
                    setChatOutput(prev => {
                        const updated = prev.map(m =>
                            m.type === 'tool_call' && m.id === event.id
                                ? { ...m, status: 'done' as const }
                                : m
                        );
                        return [...updated,
                        { type: 'tool_result' as const, id: event.id, output: event.output, success: event.success }
                        ];
                    });
                    break;
                case 'done':
                    setIsProcessing(false);
                    setAgentState('idle');
                    break;
                case 'error':
                    setChatOutput(prev => [...prev, { type: 'error', text: event.message }]);
                    setIsProcessing(false);
                    setAgentState('idle');
                    break;
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
        if (!agentModel || isProcessing) return;
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
    const handleChatSendInternal = useCallback(async (message: string) => {
        if (!agentModel || isProcessing || !message.trim()) return;
        const modelData = models.find(m => m.internalId === agentModel);
        if (!modelData) return;

        setIsProcessing(true);
        setChatOutput(prev => [...prev, { type: 'user', text: message.trim() }]);

        try {
            await api.sendAgentMessage({
                message: message.trim(),
                model_id: modelData.internalId,
                // Use correct URL for selected provider
                base_url: modelData.baseUrl || modelData.anthropicUrl || '',
                api_key: modelData.apiKey,
                model_name: modelData.modelId || modelData.name,
                // Prefer OpenAI if baseUrl exists; only use anthropic when ONLY anthropicUrl is set
                provider: (!modelData.baseUrl && modelData.anthropicUrl) ? 'anthropic' : 'openai',
                proxy_url: modelData.proxyUrl,
                server_ids: selectedServerId === 'local' ? [] : [selectedServerId],
                skills: pendingSkills.map(s => s.name),
            });
        } catch (e) {
            setChatOutput(prev => [...prev, { type: 'error', text: String(e) }]);
            setIsProcessing(false);
        }
    }, [agentModel, models, isProcessing, selectedServerId, pendingSkills]);

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
            pendingSkills, addPendingSkill, removePendingSkill,
            sshServers, addSSHServer, removeSSHServer,
            selectedServerId, selectServer,
            clearChat: () => {
                setChatOutput([]);
                api.resetAgent(selectedServerId).catch(() => { });
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
        pendingSkills, addPendingSkill, removePendingSkill,
        sshServers, selectedServerId,
        clearChat,
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

    // Wrap handleChatSend to append pending model info as text
    const localSend = useCallback(() => {
        if (pendingModels.length > 0) {
            const modelInfo = pendingModels.map(pm => {
                const md = models.find(m => m.internalId === pm.id);
                if (!md) return `- ${pm.name}`;
                const urls = [
                    md.baseUrl ? `baseUrl: ${md.baseUrl}` : '',
                    md.anthropicUrl ? `anthropicUrl: ${md.anthropicUrl}` : '',
                ].filter(Boolean).join(', ');
                return `- ${pm.name} (model: ${md.modelId || md.name}, ${urls}, apiKey: ${md.apiKey})`;
            }).join('\n');
            const userText = chatInput.trim();
            const fullMsg = (userText ? userText + '\n\n' : '') + `[Attached models for deployment]\n${modelInfo}`;
            setPendingModels([]);
            setChatInput('');
            sendMessage(fullMsg);
        } else {
            handleChatSend();
        }
    }, [pendingModels, models, chatInput, setChatInput, handleChatSend, sendMessage]);

    // Skills popup state
    const [showSkillsPicker, setShowSkillsPicker] = useState(false);
    const [showProcess, setShowProcess] = useState(true);
    const [skillsFavorites, setSkillsFavorites] = useState<Array<{ id: string; name: string; github: string }>>([]);
    const [skillsPage, setSkillsPage] = useState(0);
    const skillsPickerRef = useRef<HTMLDivElement>(null!);
    const SKILLS_PER_PAGE = 6;

    const openSkillsPicker = () => {
        setShowSkillsPicker(prev => !prev);
        if (!showSkillsPicker) {
            setSkillsPage(0);
            Promise.all([api.loadSkillsFavorites(), api.loadSkillsData(), api.loadSkillsI18n()])
                .then(([favData, skillsData, i18nMap]) => {
                    const favIds = favData.favorites || [];
                    const allSkills = (skillsData?.skills || []) as any[];
                    const skills = favIds.map(id => {
                        // Try to find actual skill data
                        const skillData = allSkills.find((s: any) => s.i === id);
                        let name = '';
                        if (skillData) {
                            // Check i18n overlay first
                            const tr = i18nMap[id];
                            name = (tr && tr.locale === locale && tr.n) ? tr.n : (skillData.n || skillData.name || id);
                        } else {
                            // Fallback: parse from path
                            const parts = id.split('/');
                            const fileName = parts[parts.length - 1].replace(/\.md$/i, '');
                            name = fileName === 'SKILL' ? (parts[parts.length - 2] || 'Unknown') : fileName;
                        }
                        return { id, name, github: id };
                    });
                    setSkillsFavorites(skills);
                })
                .catch(() => setSkillsFavorites([]));
        }
    };

    // Close skills picker on outside click
    useEffect(() => {
        if (!showSkillsPicker) return;
        const handler = (e: MouseEvent) => {
            if (skillsPickerRef.current && !skillsPickerRef.current.contains(e.target as Node)) {
                setShowSkillsPicker(false);
            }
        };
        document.addEventListener('mousedown', handler);
        return () => document.removeEventListener('mousedown', handler);
    }, [showSkillsPicker]);

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
        // Load remote hint buttons
        fetch('https://echobird.ai/api/mother/hints.json')
            .then(r => r.json())
            .then(data => setRemoteHints(data.hints || []))
            .catch(() => { /* offline: no hints shown */ });
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

    const handleScroll = () => {
        const container = chatContainerRef.current;
        if (!container) return;
        const isAtBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
        autoFollowRef.current = isAtBottom;
        setShowScrollBtn(!isAtBottom && chatOutput.length > 0);
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
                <div ref={chatContainerRef} onScroll={handleScroll} className={`absolute inset-0 ${agentModel ? 'overflow-y-auto' : 'overflow-hidden'} bg-cyber-terminal font-mono text-sm space-y-0.5 custom-scrollbar p-4 rounded-lg`}>
                    {/* Welcome banner */}
                    <div className="mb-3 select-none">
                        <div className="flex items-start justify-between gap-4 py-2">
                            {/* Left: icon + title + IP + model */}
                            <div className="flex items-center gap-4">
                                <img src="./ico-blue.svg" alt="Mother Agent" className="w-14 h-14 flex-shrink-0 drop-shadow-[0_0_6px_rgba(0,212,255,0.3)]" />
                                <div className="w-px h-12 bg-gradient-to-b from-transparent via-cyber-accent-secondary/30 to-transparent flex-shrink-0" />
                                <div className="font-mono text-xs space-y-1">
                                    <div className="text-cyber-accent-secondary text-sm font-bold tracking-wide">Mother Agent <span className="text-cyber-text-muted/60 text-xs font-normal">v{__APP_VERSION__}</span></div>
                                    <div className="text-cyber-text-muted/60">
                                        {t('mother.deployHint')}
                                    </div>
                                </div>
                            </div>
                        </div>
                        {/* Quick prompt hints — loaded from remote, scrolls with content */}
                        {remoteHints.length > 0 && (
                            <div className="flex flex-wrap gap-2 mt-2 mb-1">
                                {remoteHints.map((hint, i) => {
                                    const i18nKey = `mother.hint${hint.action[0].toUpperCase()}${hint.action.slice(1)}` as any;
                                    const label = t(i18nKey).replace('{agent}', hint.agent || '');
                                    return (
                                        <button
                                            key={i}
                                            onClick={() => { if (!agentModel) return; setChatInput(label); chatInputRef.current?.focus(); }}
                                            className={`px-3 py-1 text-xs rounded-full border border-cyber-accent-secondary/20 text-cyber-accent-secondary/70 hover:bg-cyber-accent-secondary/10 hover:text-cyber-accent-secondary transition-all ${agentModel ? 'cursor-pointer' : 'opacity-30 cursor-not-allowed'}`}
                                        >
                                            {label}
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                        <div className="text-cyber-accent-secondary/15 text-xs font-mono mt-1">{'─'.repeat(52)}</div>
                    </div>

                    {/* Chat messages */}
                    {agentModel ? (
                        <div className="space-y-1">
                            {chatOutput.filter(msg => showProcess || (msg.type !== 'tool_call' && msg.type !== 'tool_result' && msg.type !== 'thinking')).map((msg, i) => {
                                switch (msg.type) {
                                    case 'user':
                                        return <p key={i} className="break-words whitespace-pre-wrap text-white">&gt; {msg.text}</p>;
                                    case 'assistant':
                                        return <div key={i} className="break-words text-cyber-text-muted/80 channel-markdown"><ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>{msg.text}</ReactMarkdown></div>;
                                    case 'thinking':
                                        return (
                                            <div key={i} className="text-xs border-l-2 border-purple-400/30 pl-2 my-1 max-h-24 overflow-y-auto">
                                                <span className="text-purple-400/60 font-mono">💭 </span>
                                                <span className="text-purple-400/50 italic whitespace-pre-wrap">{msg.text.slice(0, 300)}{msg.text.length > 300 ? '...' : ''}</span>
                                            </div>
                                        );
                                    case 'tool_call':
                                        return (
                                            <div key={i} className="text-cyber-accent-secondary/70 font-mono text-xs border-l-2 border-cyber-accent-secondary/30 pl-2 my-1">
                                                {msg.status === 'running' ? '⟳' : '✓'} <span className="text-cyber-accent-secondary">{msg.name}</span>
                                                {msg.args && <span className="text-cyber-text-muted/50 ml-1">({msg.args.slice(0, 80)}{msg.args.length > 80 ? '...' : ''})</span>}
                                            </div>
                                        );
                                    case 'tool_result':
                                        return (
                                            <div key={i} className={`font-mono text-xs border-l-2 pl-2 my-1 max-h-32 overflow-y-auto ${msg.success ? 'border-green-500/30 text-green-400/70' : 'border-red-500/30 text-red-400/70'}`}>
                                                <pre className="whitespace-pre-wrap">{msg.output.slice(0, 500)}{msg.output.length > 500 ? '\n...' : ''}</pre>
                                            </div>
                                        );
                                    case 'error':
                                        return <p key={i} className="break-words whitespace-pre-wrap text-red-400">{msg.text}</p>;
                                    case 'state':
                                        return null; // State changes don't render as messages
                                    default:
                                        return null;
                                }
                            })}
                            {isProcessing && (
                                <p className="text-cyber-accent-secondary font-mono flex items-center gap-1">
                                    {agentState === 'executing' ? t('mother.executing') : agentState === 'tool_calling' ? t('mother.callingTool') : t('mother.processing')}
                                    <span className="inline-flex gap-[2px] ml-0.5">
                                        {[0, 1, 2].map(i => (
                                            <span
                                                key={i}
                                                className="inline-block w-1.5 h-1.5 rounded-full bg-cyber-accent-secondary"
                                                style={{
                                                    animation: 'dotPulse 1.2s ease-in-out infinite',
                                                    animationDelay: `${i * 0.2}s`,
                                                }}
                                            />
                                        ))}
                                    </span>
                                </p>
                            )}
                            <div ref={chatEndRef} />
                        </div>
                    ) : (
                        <div className="flex items-center justify-center" style={{ minHeight: 'calc(100% - 160px)' }}>
                            <div className="font-mono text-center space-y-3 select-none">
                                <div className="text-lg text-cyber-accent-secondary/40 tracking-wider">{t('mother.awaitingInit')}</div>
                                <div className="text-base text-cyber-text-muted/50 tracking-wide">{t('mother.flowHint')}</div>
                            </div>
                        </div>
                    )}
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
            <div className="flex-shrink-0 mt-3 mb-2">
                <div className="bg-cyber-terminal rounded-lg">
                    {/* Pending attachments chips */}
                    {(pendingFiles.length > 0 || pendingModels.length > 0 || pendingSkills.length > 0) && (
                        <div className="flex flex-wrap gap-2 px-3 pt-2.5 pb-1">
                            {pendingFiles.map(f => (
                                <div
                                    key={f.id}
                                    className="relative group flex items-center gap-1.5 bg-cyber-bg/60 border border-cyber-border/30 rounded px-2 py-1 h-10 text-xs font-mono text-cyber-text-muted"
                                >
                                    {f.type === 'image' && f.preview ? (
                                        <img src={f.preview} alt={f.name} className="w-6 h-6 object-cover rounded flex-shrink-0" />
                                    ) : (
                                        <Paperclip size={14} className="text-cyber-accent-secondary/60 flex-shrink-0" />
                                    )}
                                    <span className="max-w-[140px] truncate">{f.name}</span>
                                    <button
                                        onClick={() => setPendingFiles(prev => prev.filter(x => x.id !== f.id))}
                                        className="ml-0.5 text-cyber-text-muted/40 hover:text-red-400 transition-colors"
                                    >
                                        <X size={12} />
                                    </button>
                                </div>
                            ))}
                            {pendingModels.map(m => {
                                const icon = getModelIcon(m.name, m.modelId || '');
                                return (
                                    <div
                                        key={m.id}
                                        className="relative group flex items-center gap-1.5 bg-cyber-accent/5 border border-cyber-accent/30 rounded px-2 py-1 h-10 text-xs font-mono text-cyber-accent"
                                    >
                                        {icon ? (
                                            <img src={icon} alt="" className="w-6 h-6 flex-shrink-0" />
                                        ) : (
                                            <KeyRound size={14} className="text-cyber-accent/60 flex-shrink-0" />
                                        )}
                                        <span className="max-w-[160px] truncate">{m.name}</span>
                                        <button
                                            onClick={() => setPendingModels(prev => prev.filter(x => x.id !== m.id))}
                                            className="ml-0.5 text-cyber-accent/40 hover:text-red-400 transition-colors"
                                        >
                                            <X size={12} />
                                        </button>
                                    </div>
                                );
                            })}
                            {pendingSkills.map(skill => (
                                <div
                                    key={skill.id}
                                    className="relative group flex items-center gap-1.5 bg-cyber-warning/5 border border-cyber-warning/30 rounded px-2 py-1 h-10 text-xs font-mono text-cyber-warning"
                                >
                                    <Zap size={14} className="text-cyber-warning/60 flex-shrink-0" />
                                    <span className="max-w-[120px] truncate">{skill.name}</span>
                                    <button
                                        onClick={() => removePendingSkill(skill.id)}
                                        className="ml-0.5 text-cyber-warning/40 hover:text-red-400 transition-colors"
                                    >
                                        <X size={12} />
                                    </button>
                                </div>
                            ))}
                        </div>
                    )}
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
                        placeholder={agentModel ? t('mother.enterMessage') : t('mother.selectModel')}
                        disabled={!agentModel || isProcessing}
                        rows={3}
                        className="w-full bg-transparent px-4 py-2 text-sm text-cyber-text font-mono outline-none placeholder:text-cyber-text-muted/50 disabled:opacity-30 resize-none"
                    />
                    {/* Bottom toolbar */}
                    <div className="flex items-center justify-between px-3 py-1.5 border-t border-cyber-border/10">
                        <div className="flex items-center gap-1 relative">
                            <button
                                onClick={() => fileInputRef.current?.click()}
                                disabled={!agentModel || isProcessing}
                                className="p-1 text-cyber-accent-secondary/40 hover:text-cyber-accent-secondary transition-colors disabled:opacity-20"
                            >
                                <Paperclip size={15} />
                            </button>
                            <button
                                onClick={() => imageInputRef.current?.click()}
                                disabled={!agentModel || isProcessing}
                                className="p-1 text-cyber-accent-secondary/40 hover:text-cyber-accent-secondary transition-colors disabled:opacity-20"
                            >
                                <ImageIcon size={15} />
                            </button>
                            <button
                                onClick={() => setShowModelPicker(prev => !prev)}
                                disabled={!agentModel || isProcessing}
                                className={`p-1 transition-colors disabled:opacity-20 ${showModelPicker ? 'text-cyber-accent-secondary' : 'text-cyber-accent-secondary/40 hover:text-cyber-accent-secondary'}`}
                            >
                                <KeyRound size={15} />
                            </button>
                            {/* Model picker popover */}
                            {showModelPicker && (
                                <div
                                    ref={modelPickerRef}
                                    className="absolute bottom-full left-0 mb-2 w-64 max-h-48 overflow-y-auto bg-cyber-bg border border-cyber-border/60 rounded-lg shadow-lg z-50 custom-scrollbar"
                                >
                                    {models.length === 0 ? (
                                        <div className="px-3 py-2 text-xs text-cyber-text-muted/50 font-mono">{t('mother.noModels')}</div>
                                    ) : (
                                        models.map(m => (
                                            <button
                                                key={m.internalId}
                                                onClick={() => {
                                                    if (!pendingModels.some(pm => pm.id === m.internalId)) {
                                                        setPendingModels(prev => [...prev, { id: m.internalId, name: m.name, modelId: m.modelId }]);
                                                    }
                                                    setShowModelPicker(false);
                                                }}
                                                className={`w-full text-left px-3 py-2 text-xs font-mono hover:bg-cyber-accent-secondary/10 transition-colors border-b border-cyber-border/10 last:border-b-0 ${agentModel === m.internalId ? 'bg-cyber-accent-secondary/15 text-cyber-accent-secondary' : 'text-cyber-text'
                                                    }`}
                                            >
                                                <div className="font-bold truncate">{m.name}</div>
                                                <div className="text-cyber-text-muted/50 truncate text-[10px]">{m.modelId || m.baseUrl}</div>
                                            </button>
                                        ))
                                    )}
                                </div>
                            )}
                            <button
                                onClick={openSkillsPicker}
                                disabled={!agentModel || isProcessing}
                                className={`p-1 transition-colors disabled:opacity-20 ${showSkillsPicker ? 'text-cyber-warning' : 'text-cyber-warning/40 hover:text-cyber-warning'}`}
                            >
                                <Zap size={15} />
                            </button>
                            {/* Skills picker popover */}
                            {showSkillsPicker && (
                                <div
                                    ref={skillsPickerRef}
                                    className="absolute bottom-full left-0 mb-2 w-72 bg-cyber-bg border border-cyber-border/60 rounded-lg shadow-lg z-50"
                                >
                                    {skillsFavorites.length === 0 ? (
                                        <div className="px-3 py-3 text-xs text-cyber-text-muted/50 font-mono text-center">{t('mother.noFavorites')}</div>
                                    ) : (
                                        <>
                                            <div className="max-h-56 overflow-y-auto custom-scrollbar">
                                                {skillsFavorites.slice(skillsPage * SKILLS_PER_PAGE, (skillsPage + 1) * SKILLS_PER_PAGE).map(skill => (
                                                    <button
                                                        key={skill.id}
                                                        onClick={() => {
                                                            addPendingSkill({ id: skill.id, name: skill.name, github: skill.github });
                                                            setShowSkillsPicker(false);
                                                        }}
                                                        className="w-full text-left px-3 py-2 text-xs font-mono hover:bg-cyber-warning/10 transition-colors border-b border-cyber-border/10 last:border-b-0 flex items-center gap-2"
                                                    >
                                                        <Zap size={12} className="text-cyber-warning/60 flex-shrink-0" />
                                                        <div className="min-w-0">
                                                            <div className="text-cyber-warning font-bold truncate">{skill.name}</div>
                                                            <div className="text-cyber-text-muted/50 truncate text-[10px]">.../{skill.github.split(/[\/\\]/).slice(-3).join('/')}</div>
                                                        </div>
                                                    </button>
                                                ))}
                                            </div>
                                            {/* Pagination */}
                                            {Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE) > 1 && (
                                                <div className="flex items-center justify-between px-3 py-1.5 border-t border-cyber-border/20 text-[10px] font-mono text-cyber-text-muted/50">
                                                    <button
                                                        onClick={() => setSkillsPage(p => Math.max(0, p - 1))}
                                                        disabled={skillsPage === 0}
                                                        className="hover:text-cyber-warning disabled:opacity-30 transition-colors"
                                                    >
                                                        &lt; PREV
                                                    </button>
                                                    <span>{skillsPage + 1} / {Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE)}</span>
                                                    <button
                                                        onClick={() => setSkillsPage(p => Math.min(Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE) - 1, p + 1))}
                                                        disabled={skillsPage >= Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE) - 1}
                                                        className="hover:text-cyber-warning disabled:opacity-30 transition-colors"
                                                    >
                                                        NEXT &gt;
                                                    </button>
                                                </div>
                                            )}
                                        </>
                                    )}
                                </div>
                            )}
                        </div>
                        <div className="flex items-center gap-2">
                            <button
                                onClick={() => { clearChat(); }}
                                disabled={isProcessing || chatOutput.length === 0}
                                className="p-1 text-cyber-accent-secondary/40 hover:text-red-400 transition-colors disabled:opacity-20"
                            >
                                <Trash2 size={15} />
                            </button>
                            <button
                                onClick={() => setShowProcess(prev => !prev)}
                                disabled={!agentModel || isProcessing}
                                className={`flex items-center gap-2 text-xs font-mono px-1.5 py-0.5 rounded border transition-colors disabled:opacity-20 ${showProcess ? 'border-cyber-accent-secondary/40 text-cyber-accent-secondary/80 bg-cyber-accent-secondary/5' : 'border-cyber-border/30 text-cyber-text-muted/40 hover:text-cyber-text-muted/60'}`}
                            >
                                <span className={`inline-block w-2 h-2 rounded-full border transition-colors ${showProcess ? 'bg-cyber-accent-secondary border-cyber-accent-secondary' : 'border-cyber-text-muted/40'}`} />
                                {t('common.showProcess')}
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
                                    onClick={() => api.abortAgent(selectedServerId)}
                                    className="p-1 text-red-400/80 hover:text-red-400 transition-colors"
                                >
                                    <Square size={16} fill="currentColor" />
                                </button>
                            ) : (
                                <button
                                    onClick={localSend}
                                    disabled={!chatInput.trim() || !agentModel}
                                    className="p-1 text-cyber-accent-secondary/60 hover:text-cyber-accent-secondary transition-colors disabled:opacity-15"
                                >
                                    <Send size={16} />
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

// ===== Right Panel (aside area) �?SERVERS / SKILLS =====
export function MotherAgentPanel() {
    const { setChatInput, chatInputRef, addPendingSkill, sshServers, addSSHServer, removeSSHServer, selectedServerId, selectServer, isProcessing } = useMotherAgent();
    const confirm = useConfirm();
    const { t } = useI18n();

    const [panelTab, setPanelTab] = useState<'servers' | 'guide'>('servers');
    const [favoriteSkills, setFavoriteSkills] = useState<Array<{ id: string; name: string; desc: string; github: string; branch: string }>>([]);
    const [skillsLoading, setSkillsLoading] = useState(false);
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
            <div className="flex-1 p-2 overflow-y-auto">
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

