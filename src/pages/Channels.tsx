// Channels — OpenClaw agent chat interface (bridge CLI + SSH)
import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Send, CornerDownLeft, X, Square, Paperclip, Image as ImageIcon, Trash2, KeyRound, Zap, Server, ChevronsDown } from 'lucide-react';
import { MiniSelect } from '../components/MiniSelect';
import { RemoteLlmModal } from '../components/RemoteLlmModal';
import { getModelIcon } from '../components/cards/ModelCard';
import { useChannelGateway, useGatewayManager } from '../contexts/GatewayContext';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import type { ModelConfig } from '../api/types';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

// Markdown components config (stable reference, no re-creation per render)
const mdComponents = {
    code: ({ className, children, ...props }: any) => {
        const isInline = !className;
        return isInline ? (
            <code className="bg-cyber-accent/10 text-cyber-accent px-1.5 py-0.5 rounded text-[0.85em] font-mono" {...props}>{children}</code>
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
                        btn.insertAdjacentHTML('beforeend', '<span class="copy-ok" style="color:#00ff9d">✓</span>');
                        setTimeout(() => {
                            const ok = btn.querySelector('.copy-ok');
                            if (ok) ok.remove();
                            if (svg) svg.style.display = '';
                        }, 1500);
                    }}
                    className="absolute top-2 right-2 w-6 h-6 flex items-center justify-center text-cyber-text-muted/40 hover:text-cyber-accent transition-colors"
                ><svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" /></svg></button>
            </div>
        );
    },
    a: ({ href, children }: any) => (
        <a href={href} className="text-cyber-accent hover:underline" onClick={(e: React.MouseEvent) => { e.preventDefault(); api.openExternal(href); }}>{children}</a>
    ),
    strong: ({ children }: any) => <strong className="text-cyber-text-primary font-bold">{children}</strong>,
    em: ({ children }: any) => <em className="text-cyber-accent/80">{children}</em>,
    ul: ({ children }: any) => <ul className="list-disc list-inside my-1 space-y-0.5">{children}</ul>,
    ol: ({ children }: any) => <ol className="list-decimal list-inside my-1 space-y-0.5">{children}</ol>,
    h1: ({ children }: any) => <h1 className="text-lg font-bold text-cyber-text-primary mt-3 mb-1">{children}</h1>,
    h2: ({ children }: any) => <h2 className="text-base font-bold text-cyber-text-primary mt-2 mb-1">{children}</h2>,
    h3: ({ children }: any) => <h3 className="text-sm font-bold text-cyber-text-primary mt-2 mb-1">{children}</h3>,
    blockquote: ({ children }: any) => <blockquote className="border-l-2 border-cyber-accent/40 pl-3 my-1 text-cyber-text-muted/60 italic">{children}</blockquote>,
    hr: () => <hr className="border-cyber-border/30 my-2" />,
    table: ({ children }: any) => <table className="border-collapse my-2 text-sm w-full">{children}</table>,
    th: ({ children }: any) => <th className="border border-cyber-border/30 px-2 py-1 bg-cyber-accent/5 text-left font-bold">{children}</th>,
    td: ({ children }: any) => <td className="border border-cyber-border/30 px-2 py-1">{children}</td>,
};

// Memoized message component — only re-renders when msg content/role changes
const ChannelMessage = React.memo(({ role, content, toolName, toolArgs, toolSuccess }: { role: string; content: string; toolName?: string; toolArgs?: string; toolSuccess?: boolean }) => {
    if (role === 'user') {
        return <p className="break-words whitespace-pre-wrap text-white">{`> ${content}`}</p>;
    }
    if (role === 'system') {
        return <p className="break-words whitespace-pre-wrap text-red-400">{content}</p>;
    }
    if (role === 'tool_call') {
        return (
            <div className="text-cyber-accent/70 font-mono text-xs border-l-2 border-cyber-accent/30 pl-2 my-1">
                ⟳ <span className="text-cyber-accent">{toolName || content}</span>
                {toolArgs && <span className="text-cyber-text-muted/50 ml-1">({toolArgs.slice(0, 80)}{toolArgs.length > 80 ? '...' : ''})</span>}
            </div>
        );
    }
    if (role === 'tool_result') {
        return (
            <div className={`font-mono text-xs border-l-2 pl-2 my-1 max-h-32 overflow-y-auto ${toolSuccess !== false ? 'border-green-500/30 text-green-400/70' : 'border-red-500/30 text-red-400/70'}`}>
                <pre className="whitespace-pre-wrap">{content.slice(0, 500)}{content.length > 500 ? '\n...' : ''}</pre>
            </div>
        );
    }
    if (role === 'thinking') {
        return (
            <div className="text-xs border-l-2 border-purple-400/30 pl-2 my-1 max-h-24 overflow-y-auto">
                <span className="text-purple-400/60 font-mono">💭 </span>
                <span className="text-purple-400/50 italic whitespace-pre-wrap">{content.slice(0, 300)}{content.length > 300 ? '...' : ''}</span>
            </div>
        );
    }
    return (
        <div className="break-words text-cyber-text-muted/80 channel-markdown">
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>{content}</ReactMarkdown>
        </div>
    );
});
ChannelMessage.displayName = 'ChannelMessage';

// Connection protocols (display as SSH since connections go through SSH port forwarding)
const PROTOCOLS = [
    { id: 'ws://', label: 'ssh://' },
    { id: 'wss://', label: 'ssh://' },
];

// Attachment type
interface Attachment {
    type: 'image' | 'file';
    name: string;
    data: string; // base64 data URL (image) or text content (file)
    preview?: string; // Image thumbnail URL
}

interface Channel {
    id: number;
    name: string;
    address: string;
    protocol: string;
    serverId?: string; // SSH server id — used to persist alias changes
}



// Parse address into tunnelUrl + token + password
const parseAddress = (channel: Channel) => {
    let addr = channel.address.trim();
    // Strip protocol prefix if user pasted a full URL
    addr = addr.replace(/^wss?:\/\//i, '');
    const fullUrl = `${channel.protocol}${addr}`;
    const urlObj = (() => {
        try { return new URL(fullUrl); } catch { return null; }
    })();
    const token = urlObj?.searchParams.get('token')
        || urlObj?.searchParams.get('Token')
        || addr.match(/[?&]token=([^&]+)/i)?.[1]
        || '';
    const password = urlObj?.searchParams.get('password')
        || urlObj?.searchParams.get('Password')
        || addr.match(/[?&]password=([^&]+)/i)?.[1]
        || '';
    // Keep full URL (with query params) — OpenClaw requires token in URL for scope auth
    const tunnelUrl = `${channel.protocol}${addr}`;
    return { tunnelUrl, token, password };
};

// Image compression params (ref: webclaw)
const MAX_IMAGE_DIMENSION = 1280;
const IMAGE_QUALITY = 0.75;
const TARGET_IMAGE_SIZE = 300 * 1024; // SSH tunnel limit 512KB, base64 adds ~33%

// Compress image to dataURL (Canvas API)
async function compressImageToDataURL(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
        const img = new Image();
        const objectUrl = URL.createObjectURL(file);
        img.onload = () => {
            try {
                let width = img.width;
                let height = img.height;
                // Scale down to max dimension
                if (width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION) {
                    if (width > height) {
                        height = Math.round((height * MAX_IMAGE_DIMENSION) / width);
                        width = MAX_IMAGE_DIMENSION;
                    } else {
                        width = Math.round((width * MAX_IMAGE_DIMENSION) / height);
                        height = MAX_IMAGE_DIMENSION;
                    }
                }
                const canvas = document.createElement('canvas');
                canvas.width = width;
                canvas.height = height;
                const ctx = canvas.getContext('2d');
                if (!ctx) { URL.revokeObjectURL(objectUrl); reject(new Error('Canvas error')); return; }
                ctx.drawImage(img, 0, 0, width, height);
                // Keep PNG for transparency, use JPEG otherwise
                const outputType = file.type === 'image/png' ? 'image/png' : 'image/jpeg';
                let quality = IMAGE_QUALITY;
                let dataUrl = canvas.toDataURL(outputType, quality);
                // Progressively reduce quality until size limit met
                if (outputType === 'image/jpeg') {
                    const targetSize = TARGET_IMAGE_SIZE * 1.37; // base64 overhead
                    while (dataUrl.length > targetSize && quality > 0.3) {
                        quality -= 0.1;
                        dataUrl = canvas.toDataURL(outputType, quality);
                    }
                }
                URL.revokeObjectURL(objectUrl);
                resolve(dataUrl);
            } catch (e) {
                URL.revokeObjectURL(objectUrl);
                reject(e);
            }
        };
        img.onerror = () => { URL.revokeObjectURL(objectUrl); reject(new Error('Image load failed')); };
        img.src = objectUrl;
    });
}

export const Channels: React.FC = () => {
    const { t, locale } = useI18n();
    const [channels, setChannels] = useState<Channel[]>([]);
    const [activeId, setActiveId] = useState<number | null>(null);
    const [input, setInput] = useState('');
    const [arrowIndex, setArrowIndex] = useState(0);
    const [attachments, setAttachments] = useState<Attachment[]>([]);
    const [selectedModel, setSelectedModel] = useState<ModelConfig | null>(null);
    const [showModelPicker, setShowModelPicker] = useState(false);
    const [modelList, setModelList] = useState<ModelConfig[]>([]);
    const [remoteCopied, setRemoteCopied] = useState('');
    const modelPickerRef = useRef<HTMLDivElement>(null);
    // Skills/favorites picker
    const [showSkillsPicker, setShowSkillsPicker] = useState(false);
    const [skillsFavorites, setSkillsFavorites] = useState<Array<{ id: string; name: string; github: string }>>([]);
    const skillsPickerRef = useRef<HTMLDivElement>(null);
    // Process toggle (show/hide tool calls and thinking)
    const [showProcess, setShowProcess] = useState(true);

    // Bridge mode state — per-channel storage
    type BridgeMsg = { role: string; content: string; meta?: { model?: string; tokens?: number; duration_ms?: number } };
    const [allBridgeMessages, setAllBridgeMessages] = useState<Record<number, BridgeMsg[]>>({});
    const [allBridgeSessionIds, setAllBridgeSessionIds] = useState<Record<number, string>>({});
    const [allBridgeStatus, setAllBridgeStatus] = useState<Record<number, string>>({});
    const [allBridgeAgentNames, setAllBridgeAgentNames] = useState<Record<number, string>>({});
    const [allBridgeLoading, setAllBridgeLoading] = useState<Record<number, boolean>>({});

    // Per-channel helpers
    const channelKey = activeId ?? 0;
    const bridgeMessages = allBridgeMessages[channelKey] || [];
    const bridgeSessionId = allBridgeSessionIds[channelKey];
    const bridgeConnectionStatus = allBridgeStatus[channelKey] || 'standby';
    const bridgeAgentName = allBridgeAgentNames[channelKey];
    const bridgeLoading = allBridgeLoading[channelKey] || false;
    const setBridgeLoading = (val: boolean) =>
        setAllBridgeLoading(all => ({ ...all, [channelKey]: val }));
    const setBridgeMessages = (updater: BridgeMsg[] | ((prev: BridgeMsg[]) => BridgeMsg[])) => {
        setAllBridgeMessages(all => ({
            ...all,
            [channelKey]: typeof updater === 'function' ? updater(all[channelKey] || []) : updater,
        }));
    };
    const setBridgeSessionId = (sid: string | undefined) => {
        if (sid === undefined) {
            setAllBridgeSessionIds(all => { const next = { ...all }; delete next[channelKey]; return next; });
        } else {
            setAllBridgeSessionIds(all => ({ ...all, [channelKey]: sid }));
        }
    };
    const setBridgeConnectionStatus = (status: string) => {
        setAllBridgeStatus(all => ({ ...all, [channelKey]: status }));
    };
    const setBridgeAgentName = (name: string | undefined) => {
        if (name) setAllBridgeAgentNames(all => ({ ...all, [channelKey]: name }));
    };

    const inputRef = useRef<HTMLTextAreaElement>(null);
    const scrollRef = useRef<HTMLDivElement>(null);
    const fileInputRef = useRef<HTMLInputElement>(null);
    const imageInputRef = useRef<HTMLInputElement>(null);

    // Global Gateway manager
    const manager = useGatewayManager();

    // Current channel gateway connection (for SSH channels)
    const gateway = useChannelGateway(activeId);

    const activeChannel = channels.find(c => c.id === activeId);
    // All channels use bridge mode (local = spawn, remote = SSH)
    const isBridgeMode = true;
    const isLocalChannel = activeId === 1;
    const isActiveConnected = bridgeConnectionStatus === 'connected' || bridgeConnectionStatus === 'standby';
    // Bridge standby = allow sending (will auto-start for local, SSH on-demand for remote)
    const canSendMessage = bridgeConnectionStatus === 'standby' || bridgeConnectionStatus === 'connected';
    const isLocal = activeChannel?.address?.startsWith('127.0.0.1') || activeChannel?.address === 'localhost';
    const messages = bridgeMessages;

    // Load SSH servers + channel config → populate channels
    const loadChannelData = useCallback(async (preserveActiveId?: boolean) => {
        try {
            const [sshServers, savedChannels, bridgeState] = await Promise.all([
                api.loadSSHServers(),
                api.getChannelConfig(),
                api.bridgeStatus(),
            ]);

            // Set local bridge status (key=1) — don't use setBridgeConnectionStatus which depends on activeId
            setAllBridgeStatus(all => ({ ...all, 1: bridgeState.status || 'standby' }));
            if (bridgeState.agentName) setAllBridgeAgentNames(all => ({ ...all, 1: bridgeState.agentName! }));

            // LOCAL channel — uses bridge mode (openclaw agent CLI)
            const localChannel: Channel = { id: 1, name: '', address: '127.0.0.1', protocol: 'ws://' };

            const sshChannels: Channel[] = (sshServers || []).map((srv, i) => ({
                id: i + 2,
                name: srv.alias || `${srv.username}@${srv.host}`,
                address: `${srv.username}@${srv.host}`,
                protocol: 'ws://',
                serverId: srv.id,
            }));

            const all = [localChannel, ...sshChannels];
            setChannels(all);
            if (!preserveActiveId) setActiveId(all[0].id);
        } catch (e) {
            console.error('[Channels] Failed to load data:', e);
        }
    }, []);

    useEffect(() => {
        loadChannelData();
        // Refresh when SSH servers change (added/removed in Mother Agent)
        const onServersChanged = () => loadChannelData(true);
        window.addEventListener('ssh-servers-changed', onServersChanged);
        return () => window.removeEventListener('ssh-servers-changed', onServersChanged);
    }, [loadChannelData]);

    // Note: channels.json polling removed — local channel uses bridge mode,
    // SSH channels are configured via server list

    // Save to config file on channel changes
    // Smart scroll: auto-follow unless user scrolls up
    const autoFollowRef = useRef(true);
    const [showScrollBtn, setShowScrollBtn] = useState(false);
    const chatContainerRef = useRef<HTMLDivElement>(null);

    const handleChatScroll = () => {
        const container = chatContainerRef.current;
        if (!container) return;
        const isAtBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
        autoFollowRef.current = isAtBottom;
        setShowScrollBtn(!isAtBottom && messages.length > 0);
    };

    useEffect(() => {
        if (autoFollowRef.current && scrollRef.current) {
            scrollRef.current.scrollIntoView({ behavior: 'auto' });
        }
    }, [messages]);

    useEffect(() => {
        autoFollowRef.current = true;
        setShowScrollBtn(false);
        scrollRef.current?.scrollIntoView({ behavior: 'instant' as any });
    }, [activeId]);

    const scrollToBottom = () => {
        autoFollowRef.current = true;
        setShowScrollBtn(false);
        scrollRef.current?.scrollIntoView({ behavior: 'smooth' });
    };

    // Loading animation
    useEffect(() => {
        if (!bridgeLoading) return;
        const timer = setInterval(() => {
            setArrowIndex(prev => (prev + 1) % 4);
        }, 200);
        return () => clearInterval(timer);
    }, [bridgeLoading]);

    // Focus input on connect
    useEffect(() => {
        if (isActiveConnected) inputRef.current?.focus();
    }, [isActiveConnected]);

    // Mark read on channel switch
    useEffect(() => {
        if (activeId) gateway.markRead();
    }, [activeId]);

    // Switch channel (view only, no disconnect)
    const [showRemoteLlm, setShowRemoteLlm] = useState(false);
    const [remoteLlmReachable, setRemoteLlmReachable] = useState(false);
    const [remoteLlmRunning, setRemoteLlmRunning] = useState(false);
    const [remoteLlmCollapsed, setRemoteLlmCollapsed] = useState(false);

    // Check remote LLM API reachability + running status — poll every 15s
    useEffect(() => {
        if (!activeChannel || isLocal) { setRemoteLlmReachable(false); setRemoteLlmRunning(false); return; }
        const remoteIp = (activeChannel.address || '').split('@')[1] || activeChannel.address || '';
        if (!remoteIp) return;
        let cancelled = false;
        const check = async () => {
            try {
                const res = await fetch(`http://${remoteIp}:8090/api/status`, { signal: AbortSignal.timeout(3000) });
                if (!cancelled) {
                    setRemoteLlmReachable(res.ok);
                    if (res.ok) {
                        const data = await res.json().catch(() => ({}));
                        setRemoteLlmRunning(!!data.running);
                    } else {
                        setRemoteLlmRunning(false);
                    }
                }
            } catch {
                if (!cancelled) { setRemoteLlmReachable(false); setRemoteLlmRunning(false); }
            }
        };
        check();
        const interval = setInterval(check, 15000);
        return () => { cancelled = true; clearInterval(interval); };
    }, [activeId, activeChannel, isLocal]);
    const selectChannel = useCallback((id: number) => {
        if (id === activeId) return;
        setActiveId(id);
    }, [activeId]);

    // Connect to Gateway (SSH channels only — local uses bridge)
    const handleConnect = useCallback(async () => {
        if (!activeChannel || isBridgeMode) return;
        const { tunnelUrl, token, password } = parseAddress(activeChannel);
        try {
            await gateway.connect({ url: tunnelUrl, token, password });
        } catch (e) {
            console.error('[Channels] Connection failed:', e);
        }
    }, [activeChannel, gateway, isBridgeMode]);
    // Full reset: disconnect + clear messages + clear address
    const handleReset = useCallback(async () => {
        if (!activeId) return;
        if (isBridgeMode) {
            setBridgeMessages([]);
            setBridgeSessionId(undefined);
            // Stop bridge process
            try {
                await api.bridgeStop();
            } catch (e) {
                console.error('[Channels] Failed to stop bridge:', e);
            }
            setBridgeConnectionStatus('standby');
        } else {
            gateway.reset();
        }
        setChannels(prev => prev.map(c =>
            c.id === activeId ? { ...c, address: '' } : c
        ));
    }, [activeId, gateway, isBridgeMode]);


    // Update channel address
    const updateAddress = (id: number, address: string) => {
        setChannels(prev => prev.map(c =>
            c.id === id ? { ...c, address } : c
        ));
    };

    // Update channel protocol
    const updateProtocol = (id: number, protocol: string) => {
        setChannels(prev => prev.map(c =>
            c.id === id ? { ...c, protocol } : c
        ));
    };

    // Handle address input Enter key
    const handleAddressKeyDown = useCallback((e: React.KeyboardEvent) => {
        if (e.key === 'Enter' && gateway.status !== 'connected' && gateway.status !== 'connecting') {
            e.preventDefault();
            handleConnect();
        }
    }, [gateway.status, handleConnect]);

    // Handle paste image
    const handlePaste = useCallback((e: React.ClipboardEvent) => {
        const items = e.clipboardData?.items;
        if (!items) return;
        for (const item of items) {
            if (item.type.startsWith('image/')) {
                e.preventDefault();
                const file = item.getAsFile();
                if (!file) continue;
                // Compress image asynchronously
                compressImageToDataURL(file).then(dataUrl => {
                    setAttachments(prev => [...prev, {
                        type: 'image',
                        name: file.name || `paste-${Date.now()}.png`,
                        data: dataUrl,
                        preview: dataUrl,
                    }]);
                }).catch(err => {
                    console.error('[Channels] Image compression failed:', err);
                });
            }
        }
    }, []);

    // Select file (any type)
    const handleFileSelect = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
        const files = e.target.files;
        if (!files) return;
        for (const file of files) {
            if (file.type.startsWith('image/')) {
                // Compress image before adding as attachment
                compressImageToDataURL(file).then(dataUrl => {
                    setAttachments(prev => [...prev, {
                        type: 'image',
                        name: file.name,
                        data: dataUrl,
                        preview: dataUrl,
                    }]);
                }).catch(err => {
                    console.error('[Channels] Image compression failed:', err);
                });
            } else {
                // Read non-image file as text
                const reader = new FileReader();
                reader.onload = () => {
                    setAttachments(prev => [...prev, {
                        type: 'file',
                        name: file.name,
                        data: reader.result as string,
                    }]);
                };
                reader.readAsText(file);
            }
        }
        e.target.value = ''; // Reset input
    }, []);

    // Select image
    const handleImageSelect = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
        const files = e.target.files;
        if (!files) return;
        for (const file of files) {
            compressImageToDataURL(file).then(dataUrl => {
                setAttachments(prev => [...prev, {
                    type: 'image',
                    name: file.name,
                    data: dataUrl,
                    preview: dataUrl,
                }]);
            }).catch(err => {
                console.error('[Channels] Image compression failed:', err);
            });
        }
        e.target.value = '';
    }, []);

    // Remove attachment
    const removeAttachment = useCallback((index: number) => {
        setAttachments(prev => prev.filter((_, i) => i !== index));
    }, []);

    // Open model picker
    const openModelPicker = useCallback(async () => {
        try {
            const models = await api.getModels();
            setModelList(models || []);
        } catch { setModelList([]); }
        setShowModelPicker(prev => !prev);
    }, []);

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

    // Open skills picker (load favorites)
    const openSkillsPicker = useCallback(async () => {
        if (!showSkillsPicker) {
            try {
                const [favData, skillsData, i18nMap] = await Promise.all([
                    api.loadSkillsFavorites(), api.loadSkillsData(), api.loadSkillsI18n()
                ]);
                const favIds = favData.favorites || [];
                if (favIds.length > 0) {
                    const allSkills = (skillsData?.skills || []) as any[];
                    const skills = favIds.map((id: string) => {
                        const skillData = allSkills.find((s: any) => s.i === id);
                        const tr = i18nMap[id];
                        const name = skillData
                            ? ((tr && tr.locale === locale && tr.n) ? tr.n : (skillData.n || skillData.name || id))
                            : id;
                        return { id, name, github: skillData?.i || id };
                    });
                    setSkillsFavorites(skills);
                } else {
                    setSkillsFavorites([]);
                }
            } catch { setSkillsFavorites([]); }
        }
        setShowSkillsPicker(prev => !prev);
    }, [showSkillsPicker, locale]);

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

    // Format model config as plain text for sending
    const formatModelText = (m: ModelConfig): string => {
        const lines = [
            `[MODEL CONFIG]`,
            `Name: ${m.name}`,
            `Model: ${m.modelId || 'N/A'}`,
            `Base URL: ${m.baseUrl}`,
        ];
        if (m.anthropicUrl) lines.push(`Anthropic URL: ${m.anthropicUrl}`);
        lines.push(`API Key: ${m.apiKey}`);
        if (m.proxyUrl) lines.push(`Proxy URL: ${m.proxyUrl}`);
        lines.push(`[/MODEL CONFIG]`);
        return lines.join('\n');
    };

    // Send message
    const handleSend = useCallback(async () => {
        if (!activeId) return;
        if (!canSendMessage) return;
        if (bridgeLoading) return;
        if (!input.trim() && attachments.length === 0 && !selectedModel) return;
        let text = input.trim();
        // Append model config as text
        if (selectedModel) {
            const modelText = formatModelText(selectedModel);
            text = text ? `${text}\n\n${modelText}` : modelText;
        }
        setInput('');
        setAttachments([]);
        setSelectedModel(null);

        // All channels use bridge protocol
        setBridgeMessages(prev => [...prev, { role: 'user', content: text }]);
        setBridgeLoading(true);
        try {
            if (isLocalChannel) {
                // Local channel: auto-start bridge subprocess if needed
                if (bridgeConnectionStatus !== 'connected') {
                    setBridgeConnectionStatus('connecting');
                    const startResult = await api.bridgeStart();
                    if (startResult.status === 'connected') {
                        setBridgeConnectionStatus('connected');
                        if (startResult.agentName) setBridgeAgentName(startResult.agentName);
                    } else {
                        setBridgeConnectionStatus('disconnected');
                        setBridgeMessages(prev => [...prev, { role: 'system', content: `Bridge start failed: ${startResult.error || 'Unknown error'}` }]);
                        setBridgeLoading(false);
                        return;
                    }
                }
                const result = await api.bridgeChatLocal(text, bridgeSessionId);
                if (result.session_id) setBridgeSessionId(result.session_id);
                setBridgeMessages(prev => [...prev, {
                    role: 'assistant',
                    content: result.text,
                    meta: { model: result.model, tokens: result.tokens, duration_ms: result.duration_ms },
                }]);
            } else {
                // Remote channel: SSH → bridge binary on remote server
                const serverId = activeChannel?.serverId;
                if (!serverId) {
                    setBridgeMessages(prev => [...prev, { role: 'system', content: 'No server ID found for this channel' }]);
                    setBridgeLoading(false);
                    return;
                }
                setBridgeConnectionStatus('connected');
                setBridgeAgentName('OpenClaw'); // Default agent name for remote
                const result = await api.bridgeChatRemote(serverId, text, bridgeSessionId);
                if (result.session_id) setBridgeSessionId(result.session_id);
                setBridgeMessages(prev => [...prev, {
                    role: 'assistant',
                    content: result.text,
                    meta: { model: result.model, tokens: result.tokens, duration_ms: result.duration_ms },
                }]);
            }
        } catch (e: any) {
            setBridgeMessages(prev => [...prev, { role: 'system', content: `Error: ${e?.message || e}` }]);
            if (isLocalChannel) {
                try {
                    const s = await api.bridgeStatus();
                    setBridgeConnectionStatus(s.status || 'disconnected');
                } catch { setBridgeConnectionStatus('disconnected'); }
            }
        } finally {
            setBridgeLoading(false);
        }
        inputRef.current?.focus();
    }, [activeId, input, attachments, selectedModel, isActiveConnected, canSendMessage, isLocalChannel, bridgeLoading, bridgeSessionId, bridgeConnectionStatus, activeChannel]);

    // Abort current request
    const handleAbort = useCallback(() => {
        if (!isBridgeMode) gateway.abort();
    }, [gateway, isBridgeMode]);

    return (
        <div className="flex h-full gap-0 overflow-hidden">
            {/* ======== Left: Channel list ======== */}
            <div className="w-56 flex-shrink-0 flex flex-col">
                <div className="flex-1 overflow-y-auto pr-2 py-2 space-y-2 custom-scrollbar">
                    {channels.map(ch => {
                        const isActive = activeId === ch.id;
                        const chState = manager.getChannelState(ch.id);
                        // Bridge mode: use per-channel bridgeStatus
                        const chBridgeStatus = allBridgeStatus[ch.id] || 'standby';
                        const isLinked = chBridgeStatus === 'connected';
                        const isBridgeConnecting = chBridgeStatus === 'connecting';
                        const isError = chBridgeStatus === 'disconnected';
                        const isStandby = chBridgeStatus === 'standby';
                        const hasNew = chState.hasNewMessage && !isActive;

                        return (
                            <div
                                key={ch.id}
                                onClick={() => selectChannel(ch.id)}
                                className={`w-full text-left p-3 transition-all font-mono border rounded-card cursor-pointer ${isActive
                                    ? 'border-cyber-accent bg-cyber-accent/10 shadow-cyber-card'
                                    : 'border-cyber-border shadow-cyber-card bg-black/80 hover:border-cyber-accent/30 hover:bg-black/90'
                                    }`}
                            >
                                {/* Status */}
                                <div className="flex items-center gap-1.5 mb-1.5">
                                    <div className={`w-2 h-2 rounded-full ${isLinked ? 'bg-cyber-accent animate-pulse' : isBridgeConnecting ? 'bg-yellow-400 animate-pulse' : isError ? 'bg-red-400' : 'bg-cyber-text-muted/50'}`} />
                                    <span className={`text-xs tracking-wide ${isLinked ? 'text-cyber-accent' : isBridgeConnecting ? 'text-yellow-400' : isError ? 'text-red-400' : 'text-cyber-text-muted/70'}`}>
                                        [{isLinked ? t('channel.linked') : isBridgeConnecting ? t('channel.connecting') : isError ? t('channel.failed') : t('channel.standby')}]
                                    </span>
                                    {/* Unread badge */}
                                    {hasNew && (
                                        <span className="ml-auto w-2 h-2 rounded-full bg-red-500 animate-pulse" />
                                    )}
                                </div>
                                {/* Name — read-only, edit via Mother Agent */}
                                <div className="flex items-center gap-1 h-5">
                                    <span className={`text-sm font-bold whitespace-nowrap truncate ${isActive ? 'text-cyber-accent' : 'text-cyber-accent/90'}`}>
                                        {ch.name || (ch.address?.startsWith('127.0.0.1') || ch.address === 'localhost'
                                            ? `${t('mother.local')} (127.0.0.1)`
                                            : ch.address)}
                                    </span>
                                </div>
                            </div>
                        );
                    })}
                </div>

            </div>

            {/* ======== Right: Chat area ======== */}
            <div className="flex-1 flex flex-col min-w-0 relative">
                {activeChannel && (
                    <>
                        {/* Remote LLM status bar — collapsible, only for remote hosts */}
                        {!isLocal && !remoteLlmCollapsed && (
                            <div
                                className={`mx-4 mt-2 px-4 py-2.5 rounded-card border border-cyber-border/30 bg-black/80 shadow-cyber-card cursor-pointer transition-all duration-200 hover:border-cyber-accent hover:shadow-[0_0_12px_rgba(0,255,157,0.15)] group`}
                                onClick={() => remoteLlmReachable ? setShowRemoteLlm(true) : setRemoteLlmCollapsed(true)}
                            >
                                <div className="flex items-center justify-between">
                                    <div className="flex items-center gap-2.5 font-mono text-sm">
                                        <Server size={14} className={remoteLlmRunning ? 'text-cyber-accent' : remoteLlmReachable ? 'text-red-400' : 'text-cyber-text-muted/50'} />
                                        {remoteLlmReachable ? (
                                            remoteLlmRunning ? (
                                                <span className="text-cyber-accent text-xs uppercase tracking-wider">{t('channel.remoteLlm')} {t('status.running')}</span>
                                            ) : (
                                                <span className="text-red-400 text-xs uppercase tracking-wider">{t('channel.remoteLlm')} {t('status.offline')}</span>
                                            )
                                        ) : (
                                            <span className="text-cyber-text-muted/60 text-xs tracking-wider">Mother Agent → {t('mother.hintDeployLlm')}</span>
                                        )}
                                    </div>
                                    <div className="flex items-center gap-2">
                                        {remoteLlmReachable ? (
                                            <span className="text-cyber-text-muted/50 text-xs font-mono group-hover:text-cyber-accent/80 transition-colors">
                                                {t('channel.llmPanel')}
                                            </span>
                                        ) : (
                                            <button
                                                onClick={(e) => { e.stopPropagation(); setRemoteLlmCollapsed(true); }}
                                                className="p-0.5 text-cyber-text-muted/40 hover:text-cyber-accent transition-colors"
                                                title="Collapse"
                                            >
                                                <ChevronsDown size={14} />
                                            </button>
                                        )}
                                    </div>
                                </div>
                            </div>
                        )}
                        {/* Floating circular icon when bar is collapsed */}
                        {!isLocal && remoteLlmCollapsed && (
                            <button
                                onClick={() => setRemoteLlmCollapsed(false)}
                                className="group absolute top-4 right-6 z-10 w-10 h-10 rounded-full border border-cyber-border/30 bg-black/80 shadow-cyber-card flex items-center justify-center transition-all duration-200 hover:border-cyber-accent hover:shadow-[0_0_12px_rgba(0,255,157,0.15)]"
                            >
                                <Server size={16} className="text-cyber-text-muted/40 group-hover:text-cyber-accent transition-colors duration-200" />
                            </button>
                        )}

                        {!isActiveConnected && !canSendMessage && gateway.status !== 'connecting' && bridgeConnectionStatus !== 'connecting' ? (
                            /* Awaiting deployment state — centered status */
                            <div className="flex-1 mx-4 mt-2 bg-cyber-terminal rounded-lg flex items-center justify-center">
                                <div className="text-center font-mono space-y-3 select-none max-w-md">
                                    <p className="text-cyber-text-muted/70 text-base">&gt; {t('channel.deployFirst')}</p>
                                    <p className="text-sm text-cyber-text-muted/50">
                                        {t('channel.motherFlow')}
                                    </p>
                                    {!isBridgeMode && (
                                        <p className="text-xs text-cyber-accent-secondary/50 mt-2">
                                            💡 {t('mother.hintDeployLlm')} · {t('mother.hintDeployBridge').replace('{agent}', 'OpenClaw')}
                                        </p>
                                    )}
                                </div>
                            </div>
                        ) : (
                            /* Connected / Connecting — terminal chat area */
                            <div className="relative flex-1 mx-4 mt-2">
                                <div ref={chatContainerRef} onScroll={handleChatScroll} className="absolute inset-0 overflow-y-auto bg-cyber-terminal font-mono text-sm space-y-1 custom-scrollbar p-4 rounded-lg">
                                    {/* System info */}
                                    <div className="space-y-1 select-none">
                                        <p className="text-cyber-accent">[SYS] {activeChannel.name || `Channel #${String(activeChannel.id).padStart(2, '0')}`}</p>
                                        {isBridgeMode ? (
                                            <p className="text-cyber-accent/80">{bridgeAgentName ? `${bridgeAgentName} · ` : ''}Echobird Bridge Protocol</p>
                                        ) : (
                                            <>
                                                <p className="text-cyber-accent/80">SSH · {activeChannel.address}</p>
                                                {gateway.status === 'connecting' && (
                                                    <p className="text-yellow-400">[SYS] {t('channel.connecting')}</p>
                                                )}
                                                {isActiveConnected && (
                                                    <p className="text-cyber-accent">[SYS] {t('channel.connectedTo')} · {activeChannel.address}</p>
                                                )}
                                                {gateway.status === 'error' && (
                                                    <p className="text-red-400">[SYS] {t('channel.connectionFailed')}</p>
                                                )}
                                            </>
                                        )}
                                    </div>

                                    {messages.filter(msg => showProcess || (msg.role !== 'tool_call' && msg.role !== 'tool_result' && msg.role !== 'thinking')).map((msg, i) => (
                                        <div key={i}>
                                            <ChannelMessage role={msg.role} content={msg.content} toolName={(msg as any).toolName} toolArgs={(msg as any).toolArgs} toolSuccess={(msg as any).toolSuccess} />
                                            {msg.role === 'assistant' && (msg as any).meta && (
                                                <p className="text-cyber-text-muted/40 text-xs font-mono mt-0.5 mb-1">
                                                    {(msg as any).meta.model && <span>{(msg as any).meta.model}</span>}
                                                    {(msg as any).meta.tokens && <span> · {(msg as any).meta.tokens.toLocaleString()} tokens</span>}
                                                    {(msg as any).meta.duration_ms && <span> · {((msg as any).meta.duration_ms / 1000).toFixed(1)}s</span>}
                                                </p>
                                            )}
                                        </div>
                                    ))}

                                    {(bridgeLoading) ? (
                                        <p className="text-cyber-accent font-mono">[EXEC] <span className="inline-block w-8 text-left">{['>', '>>', '>>>', ''][arrowIndex]}</span> {t('channel.transmitting')}</p>
                                    ) : isActiveConnected && (
                                        <p className="text-cyber-accent">_ ready</p>
                                    )}
                                    <div ref={scrollRef} />
                                </div>
                                {showScrollBtn && (
                                    <button
                                        onClick={scrollToBottom}
                                        className="absolute bottom-3 right-3 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-accent hover:border-cyber-accent/50 transition-colors z-10"
                                    ><svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 9l6 6 6-6" /></svg></button>
                                )}
                            </div>
                        )}

                        {/* Input area */}
                        <div className="flex-shrink-0 mx-4 mt-3 mb-2">
                            <div className="bg-cyber-terminal rounded-lg">
                                {/* Attachment preview */}
                                {(attachments.length > 0 || selectedModel) && (
                                    <div className="flex flex-wrap gap-2 px-3 pt-2">
                                        {/* Model config card */}
                                        {selectedModel && (
                                            <div className="relative group flex items-center gap-1.5 bg-cyber-accent/5 border border-cyber-accent/30 rounded px-2 py-1 min-h-[2.5rem] text-xs font-mono text-cyber-accent">
                                                {(() => {
                                                    const icon = getModelIcon(selectedModel.name, selectedModel.modelId); return icon ? (
                                                        <img src={icon} alt="" className="w-6 h-6" />
                                                    ) : (
                                                        <KeyRound size={16} className="text-cyber-accent/60" />
                                                    );
                                                })()}
                                                <span className="max-w-[160px] truncate">{selectedModel.name}</span>
                                                <button
                                                    onClick={() => setSelectedModel(null)}
                                                    className="ml-0.5 text-cyber-accent/40 hover:text-red-400 transition-colors"
                                                >
                                                    <X size={12} />
                                                </button>
                                            </div>
                                        )}
                                        {/* File/image attachments */}
                                        {attachments.map((att, i) => (
                                            <div key={i} className="relative group flex items-center gap-1.5 bg-cyber-bg/60 border border-cyber-border/30 rounded px-2 py-1 text-xs font-mono text-cyber-text-muted">
                                                {att.type === 'image' && att.preview ? (
                                                    <img src={att.preview} alt={att.name} className="w-8 h-8 object-cover rounded" />
                                                ) : (
                                                    <Paperclip size={12} className="text-cyber-accent/60" />
                                                )}
                                                <span className="max-w-[120px] truncate">{att.name}</span>
                                                <button
                                                    onClick={() => removeAttachment(i)}
                                                    className="ml-0.5 text-cyber-text-muted/40 hover:text-red-400 transition-colors"
                                                >
                                                    <X size={12} />
                                                </button>
                                            </div>
                                        ))}
                                    </div>
                                )}
                                <textarea
                                    ref={inputRef}
                                    value={input}
                                    onChange={e => setInput(e.target.value)}
                                    onKeyDown={e => {
                                        if (e.key === 'Enter' && !e.shiftKey) {
                                            e.preventDefault();
                                            handleSend();
                                        }
                                    }}
                                    onPaste={handlePaste}
                                    placeholder={(bridgeLoading) ? t('channel.awaitingResponse') : t('channel.enterMessage')}
                                    disabled={(bridgeLoading) || (!canSendMessage && !isActiveConnected)}
                                    rows={3}
                                    className="w-full bg-transparent px-4 py-2 text-sm text-cyber-text font-mono outline-none placeholder:text-cyber-text-muted/60 disabled:opacity-30 resize-none"
                                />
                                {/* Bottom toolbar */}
                                <div className="flex items-center justify-between px-3 py-1.5 border-t border-cyber-border/20">
                                    <div className="flex items-center gap-1 relative">
                                        <button
                                            onClick={() => fileInputRef.current?.click()}
                                            disabled={bridgeLoading || !isActiveConnected}
                                            className="p-1 text-cyber-accent/60 hover:text-cyber-accent transition-colors disabled:opacity-20"
                                        >
                                            <Paperclip size={15} />
                                        </button>
                                        <button
                                            onClick={() => imageInputRef.current?.click()}
                                            disabled={bridgeLoading || !isActiveConnected}
                                            className="p-1 text-cyber-accent/60 hover:text-cyber-accent transition-colors disabled:opacity-20"
                                        >
                                            <ImageIcon size={15} />
                                        </button>
                                        <button
                                            onClick={openModelPicker}
                                            disabled={bridgeLoading || !isActiveConnected}
                                            className={`p-1 transition-colors disabled:opacity-20 ${selectedModel ? 'text-cyber-accent' : 'text-cyber-accent/60 hover:text-cyber-accent'}`}
                                        >
                                            <KeyRound size={15} />
                                        </button>
                                        <button
                                            onClick={openSkillsPicker}
                                            disabled={bridgeLoading || !isActiveConnected}
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
                                                    <div className="max-h-56 overflow-y-auto custom-scrollbar">
                                                        {skillsFavorites.map(skill => (
                                                            <button
                                                                key={skill.id}
                                                                onClick={() => {
                                                                    setInput(prev => (prev ? prev + '\n' : '') + `Install skill: ${skill.name} (${skill.github})`);
                                                                    setShowSkillsPicker(false);
                                                                    inputRef.current?.focus();
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
                                                )}
                                            </div>
                                        )}
                                        {/* Model picker popover (upward) */}
                                        {showModelPicker && (
                                            <div
                                                ref={modelPickerRef}
                                                className="absolute bottom-full left-0 mb-2 w-64 max-h-48 overflow-y-auto bg-cyber-bg border border-cyber-border/60 rounded-lg shadow-lg z-50 custom-scrollbar"
                                            >
                                                {modelList.length === 0 ? (
                                                    <div className="px-3 py-2 text-xs text-cyber-text-muted/50 font-mono">{t('channel.noModels')}</div>
                                                ) : (
                                                    modelList.map(m => (
                                                        <button
                                                            key={m.internalId}
                                                            onClick={() => { setSelectedModel(m); setShowModelPicker(false); }}
                                                            className="w-full text-left px-3 py-2 text-xs font-mono hover:bg-cyber-accent/10 transition-colors border-b border-cyber-border/10 last:border-b-0 flex items-center gap-2"
                                                        >
                                                            {(() => {
                                                                const icon = getModelIcon(m.name, m.modelId); return icon ? (
                                                                    <img src={icon} alt="" className="w-5 h-5 flex-shrink-0" />
                                                                ) : (
                                                                    <KeyRound size={14} className="text-cyber-accent/40 flex-shrink-0" />
                                                                );
                                                            })()}
                                                            <div className="min-w-0">
                                                                <div className="text-cyber-accent font-bold truncate">{m.name}</div>
                                                                <div className="text-cyber-text-muted/50 truncate">{m.modelId || m.baseUrl}</div>
                                                            </div>
                                                        </button>
                                                    ))
                                                )}
                                            </div>
                                        )}
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                        <button
                                            onClick={() => setShowProcess(prev => !prev)}
                                            disabled={bridgeLoading || !isActiveConnected}
                                            className={`flex items-center gap-2 text-xs font-mono px-1.5 py-0.5 rounded border transition-colors disabled:opacity-20 ${showProcess ? 'border-cyber-accent/40 text-cyber-accent/80 bg-cyber-accent/5' : 'border-cyber-border/30 text-cyber-text-muted/40 hover:text-cyber-text-muted/60'}`}
                                        >
                                            <span className={`inline-block w-2 h-2 rounded-full border transition-colors ${showProcess ? 'bg-cyber-accent border-cyber-accent' : 'border-cyber-text-muted/40'}`} />
                                            {t('common.showProcess')}
                                        </button>
                                        {bridgeLoading ? (
                                            <button
                                                onClick={handleAbort}
                                                className="p-1 text-red-400 hover:text-red-300 transition-colors"
                                            >
                                                <Square size={16} fill="currentColor" />
                                            </button>
                                        ) : (
                                            <button
                                                onClick={handleSend}
                                                disabled={(!input.trim() && attachments.length === 0 && !selectedModel) || !isActiveConnected}
                                                className="p-1 text-cyber-accent/60 hover:text-cyber-accent transition-colors disabled:opacity-15"
                                            >
                                                <Send size={16} />
                                            </button>
                                        )}
                                    </div>
                                </div>
                                {/* Hidden file inputs */}
                                <input ref={fileInputRef} type="file" multiple className="hidden" onChange={handleFileSelect} />
                                <input ref={imageInputRef} type="file" multiple accept="image/*" className="hidden" onChange={handleImageSelect} />
                            </div>
                        </div>
                    </>
                )
                }
            </div >
            <RemoteLlmModal
                isOpen={showRemoteLlm}
                onClose={() => setShowRemoteLlm(false)}
                remoteHost={activeChannel?.address || 'remote'}
                displayName={activeChannel?.name}
            />
        </div >
    );
};
