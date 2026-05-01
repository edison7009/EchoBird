import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Send, ChevronDown, Square } from 'lucide-react';
import { RemoteModelSelector, type ModelOption } from '../../components/RemoteModelSelector';
import { getModelIcon } from '../../components/cards/ModelCard';
import { PendingChipsRow } from '../../components/PendingChipsRow';
import { ChatBubble, ToolCallCard } from '../../components/chat';
import { buildPendingMessage } from '../../utils/buildPendingMessage';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import { useMotherAgent } from './context';
import { MA_PAGE_SIZE } from './types';

declare const __APP_VERSION__: string;

// ===== Main Content (center area) — CHAT =====
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

        sshServers, selectedServerId,
        clearChat, abortAgent,
        maDiskTotal, loadOlderChat,
    } = useMotherAgent();

    // Build model list for RemoteModelSelector (with icons)
    const modelList: ModelOption[] = React.useMemo(() =>
        models.map(m => ({ id: m.internalId, name: m.name, icon: getModelIcon(m.name, m.modelId) })),
        [models],
    );

    // Listen for clear-chat event from title bar
    useEffect(() => {
        const handler = () => clearChat();
        window.addEventListener('clear-chat', handler);
        return () => window.removeEventListener('clear-chat', handler);
    }, [clearChat]);

    const [publicIP, setPublicIP] = useState('...');
    const [remoteHints, setRemoteHints] = useState<Array<{ action: string; agent?: string }>>([]);
    const [serverModel, setServerModel] = useState<string | null>(null);
    const chatInputRef = useRef<HTMLTextAreaElement>(null!);
    const fileInputRef = useRef<HTMLInputElement>(null!);
    const imageInputRef = useRef<HTMLInputElement>(null!);


    const [pendingFiles, setPendingFiles] = useState<Array<{ id: string; name: string; type: 'file' | 'image'; preview?: string }>>([]);

    // Wrap handleChatSend to append pending file info as text
    const localSend = useCallback(() => {
        const hasFiles = pendingFiles.length > 0;

        if (hasFiles) {
            const { messageText, chips } = buildPendingMessage(
                chatInput,
                pendingFiles,
                [],
                [],
            );

            setPendingFiles([]);
            setChatInput('');
            sendMessage(messageText, chatInput.trim(), chips);
        } else {
            handleChatSend();
        }
    }, [pendingFiles, chatInput, setChatInput, handleChatSend, sendMessage]);




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
        // Load quick-hint buttons from bundled assets (offline-first).
        api.getMotherHints()
            .then(s => {
                try {
                    const data = JSON.parse(s);
                    setRemoteHints(data.hints || []);
                } catch {
                    setRemoteHints([]);
                }
            })
            .catch(() => setRemoteHints([]));
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
    const isProgrammaticScrollRef = useRef(false);
    const [showScrollBtn, setShowScrollBtn] = useState(false);
    const PAGE_SIZE = MA_PAGE_SIZE;
    const [displayCount, setDisplayCount] = useState(PAGE_SIZE);
    const [showSkeleton, setShowSkeleton] = useState(false);

    // Reset pagination when server changes
    useEffect(() => { setDisplayCount(PAGE_SIZE); }, [selectedServerId]);

    const handleScroll = () => {
        const container = chatContainerRef.current;
        if (!container) return;
        // Skip autoFollow updates during programmatic scroll
        if (isProgrammaticScrollRef.current) return;
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

    // Helper: programmatic scroll that won't flip autoFollowRef
    const doScrollToBottom = (behavior: ScrollBehavior = 'auto') => {
        isProgrammaticScrollRef.current = true;
        autoFollowRef.current = true;
        setShowScrollBtn(false);
        chatEndRef.current?.scrollIntoView({ behavior });
        setTimeout(() => { isProgrammaticScrollRef.current = false; }, 100);
    };

    useEffect(() => {
        if (autoFollowRef.current) {
            requestAnimationFrame(() => doScrollToBottom('auto'));
        }
    }, [chatOutput]);

    const scrollToBottom = () => doScrollToBottom('smooth');

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
                                            onClick={() => {
                                                const el = chatInputRef.current;
                                                if (el) {
                                                    const start = el.selectionStart ?? el.value.length;
                                                    const end = el.selectionEnd ?? start;
                                                    const before = el.value.slice(0, start);
                                                    const after = el.value.slice(end);
                                                    setChatInput(before + label + after);
                                                    el.focus();
                                                    requestAnimationFrame(() => {
                                                        const pos = start + label.length;
                                                        el.selectionStart = el.selectionEnd = pos;
                                                    });
                                                } else {
                                                    setChatInput(label);
                                                }
                                            }}
                                            className="px-3 py-1 text-xs rounded-full border border-cyber-accent/20 text-cyber-accent/70 hover:bg-cyber-accent/10 hover:text-cyber-accent transition-all cursor-pointer"
                                        >
                                            {label}
                                        </button>
                                    );
                                })}
                            </div>
                        )}
                    </div>

                    {/* Chat messages — markdown stream */}
                        <div className="pt-2 pb-2">
                            {/* Skeleton placeholders — shown briefly when lazy-loading older messages */}
                            {showSkeleton && [0, 1, 2].map(i => (
                                <ChatBubble key={`sk-${i}`} role="skeleton" content="" variant="mother" />
                            ))}
                            {chatOutput.slice(-displayCount).map((msg, i, arr) => {
                                if (msg.type === 'user') {
                                    return <ChatBubble key={i} role="user" content={msg.text} variant="mother" chips={msg.chips} />;
                                }
                                if (msg.type === 'tool_call') {
                                    return <ToolCallCard key={`${i}-${msg.id}`} name={msg.name} args={msg.args} status={msg.status} output={msg.output} />;
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
                                    const text = msg.i18nKey ? t(msg.i18nKey as import('../../i18n/types').TKey) : msg.text;
                                    return <div key={i} className="flex justify-center my-4"><span className="text-cyber-text-muted/35 text-xs font-mono">{text}</span></div>;
                                }
                                if (msg.type === 'error') {
                                    const text = msg.i18nKey ? t(msg.i18nKey as import('../../i18n/types').TKey) : msg.text;
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
                        className="absolute bottom-3 right-3 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-accent hover:border-cyber-accent/50 transition-colors z-10"
                    >
                        <ChevronDown className="w-4 h-4" />
                    </button>
                )}
            </div>

            {/* Rich input area */}
            <div className="flex-shrink-0 mt-1 mb-1">
                <div className="bg-cyber-input rounded-lg p-2">
                    {/* Pending attachments chips — shared component */}
                    <PendingChipsRow
                        files={pendingFiles}
                        onRemoveFile={id => setPendingFiles(prev => prev.filter(x => x.id !== id))}

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
                        className="w-full bg-transparent px-2 py-1 text-sm text-[#DED9D2] font-sans font-medium outline-none placeholder:text-[#DED9D2]/40 disabled:opacity-30 resize-none"
                    />
                    <div className="flex items-center justify-end gap-1.5">
                        <RemoteModelSelector
                            models={modelList}
                            currentModelId={agentModel}
                            loading={false}
                            onSelect={(id) => setAgentModel(id || null)}
                            placeholder={t('mother.selectModel')}
                        />
                        {isProcessing ? (
                            <button
                                onClick={() => abortAgent()}
                                className="w-8 h-8 rounded-lg flex items-center justify-center bg-red-500/20 hover:bg-red-500/30 transition-colors"
                            >
                                <Square size={14} fill="#f87171" className="text-red-400" />
                            </button>
                        ) : (
                            <button
                                onClick={localSend}
                                disabled={!chatInput.trim()}
                                className="w-8 h-8 rounded-lg flex items-center justify-center bg-cyber-accent hover:brightness-110 transition-all disabled:opacity-20"
                            >
                                <Send size={18} className="text-cyber-bg rotate-45 -translate-x-[1px]" />
                            </button>
                        )}
                    </div>
                </div>
                {/* Hidden file inputs */}

            </div>
        </div>
    );
}
