// Channels — OpenClaw agent chat interface (bridge CLI + SSH)
import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Send, CornerDownLeft, X, Square, Paperclip, Image as ImageIcon, RotateCcw, KeyRound, Zap, Server, ChevronsDown, ChevronLeft, ChevronRight, ArrowLeftRight } from 'lucide-react';
import { MiniSelect } from '../components/MiniSelect';
import { getModelIcon } from '../components/cards/ModelCard';
import { PendingChipsRow } from '../components/PendingChipsRow';
import { ChatBubble, TerminalStatusBar } from '../components/chat';
import { useChannelGateway, useGatewayManager } from '../contexts/GatewayContext';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import { channelHistoryLoad, channelHistorySave, channelHistoryClear } from '../api/tauri';
import { normalizeError, errorToKey } from '../utils/normalizeError';
import { buildPendingMessage } from '../utils/buildPendingMessage';
import type { ModelConfig } from '../api/types';




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
    const [pendingModels, setPendingModels] = useState<Array<{ id: string; name: string; modelId?: string }>>([]);
    const [showModelPicker, setShowModelPicker] = useState(false);
    const [modelList, setModelList] = useState<ModelConfig[]>([]);
    const [remoteCopied, setRemoteCopied] = useState('');
    const modelPickerRef = useRef<HTMLDivElement>(null);
    // Skills/favorites picker
    const [showSkillsPicker, setShowSkillsPicker] = useState(false);
    const [skillsFavorites, setSkillsFavorites] = useState<Array<{ id: string; name: string; github: string }>>([]);
    const skillsPickerRef = useRef<HTMLDivElement>(null);
    const [skillsPage, setSkillsPage] = useState(0);
    const SKILLS_PER_PAGE_CH = 4;
    const [pendingSkills, setPendingSkills] = useState<Array<{ id: string; name: string; github: string; branch: string }>>([]);
    // Process toggle (show/hide tool calls and thinking)

    // Bridge mode state — per-channel storage
    type BridgeMsg = { role: string; content: string; i18nKey?: string; meta?: { model?: string; tokens?: number; duration_ms?: number }; chips?: import('../components/chat/ChatBubble').BubbleChip[] };
    const [allBridgeMessages, setAllBridgeMessages] = useState<Record<number, BridgeMsg[]>>({});
    const [allBridgeSessionIds, setAllBridgeSessionIds] = useState<Record<number, string>>({});
    const [allBridgeStatus, setAllBridgeStatus] = useState<Record<number, string>>({});
    const [allBridgeAgentNames, setAllBridgeAgentNames] = useState<Record<number, string>>({});
    const [allBridgeLoading, setAllBridgeLoading] = useState<Record<number, boolean>>({});
    // Per-channel active agent selection (for tab switching demo)
    const AGENT_LIST = [
        { name: 'OpenClaw', icon: '/icons/tools/openclaw.svg' },
        { name: 'Claude Code', icon: '/icons/tools/claudecode.svg' },
        { name: 'OpenCode', icon: '/icons/tools/opencode.svg' },
    ];
    const [allActiveAgents, setAllActiveAgents] = useState<Record<number, string>>({});
    const setActiveAgentFor = (chId: number, name: string) => setAllActiveAgents(prev => ({ ...prev, [chId]: name }));

    // Per-channel helpers
    const channelKey = activeId ?? 0;
    const bridgeMessages = allBridgeMessages[channelKey] || [];
    const bridgeSessionId = allBridgeSessionIds[channelKey];
    const bridgeConnectionStatus = allBridgeStatus[channelKey] || 'standby';
    const bridgeAgentName = allBridgeAgentNames[channelKey];
    const bridgeLoading = allBridgeLoading[channelKey] || false;

    // ─── Stable disk key derived from channel address (computed early from channels state) ───
    // e.g. activeId=1 → 'local', SSH → 'eben_192.168.10.39'
    const _ch = channels.find(c => c.id === activeId);
    const channelFileKey: string | null = activeId == null
        ? null
        : activeId === 1
            ? 'local'
            : _ch
                ? (_ch.address || _ch.name || `ch_${activeId}`).replace(/[^a-zA-Z0-9._-]/g, '_')
                : null;

    // ─── disk total for this channel (used for lazy-load) ───
    const [diskTotal, setDiskTotal] = useState<Record<string, number>>({});
    const getDiskTotal = () => (channelFileKey ? (diskTotal[channelFileKey] ?? 0) : 0);
    const setDiskTotalFor = (key: string, n: number) => setDiskTotal(prev => ({ ...prev, [key]: n }));
    const saveDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const setBridgeLoading = (val: boolean) =>
        setAllBridgeLoading(all => ({ ...all, [channelKey]: val }));
    const setBridgeMessages = (updater: BridgeMsg[] | ((prev: BridgeMsg[]) => BridgeMsg[])) => {
        setAllBridgeMessages(all => {
            const next = typeof updater === 'function' ? updater(all[channelKey] || []) : updater;
            // Debounced save to disk (only user/assistant messages)
            if (channelFileKey) {
                const fk = channelFileKey;
                const save = next.filter(m => m.role === 'user' || m.role === 'assistant');
                if (saveDebounceRef.current) clearTimeout(saveDebounceRef.current);
                saveDebounceRef.current = setTimeout(() => {
                    channelHistorySave(fk, save.map(m => ({ role: m.role, content: m.content }))).catch(() => {});
                }, 800);
            }
            return { ...all, [channelKey]: next };
        });
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
    // Bridge standby/disconnected = allow sending (local auto-restarts, remote reconnects per-send via SSH)
    const canSendMessage = bridgeConnectionStatus !== 'connecting';
    const isLocal = activeChannel?.address?.startsWith('127.0.0.1') || activeChannel?.address === 'localhost';
    const messages = bridgeMessages;
    const prevChErrRef = useRef(0);
    const prevChActiveIdRef = useRef<number | null>(null);
    // Fire window event ONLY for genuinely new errors in the current channel session
    useEffect(() => {
        const errCount = messages.filter(m => m.role === 'system' && m.i18nKey !== 'error.userCancelled').length;
        if (activeId !== prevChActiveIdRef.current) {
            // Channel switched — sync ref without firing (existing errors are not new)
            prevChErrRef.current = errCount;
            prevChActiveIdRef.current = activeId;
        } else if (errCount > prevChErrRef.current) {
            window.dispatchEvent(new CustomEvent('chat-error', { detail: { count: errCount - prevChErrRef.current } }));
            prevChErrRef.current = errCount;
        }
    }, [messages, activeId]);

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
    const CH_PAGE_SIZE = 30;
    const [chDisplayCount, setChDisplayCount] = useState(CH_PAGE_SIZE);
    const [chShowSkeleton, setChShowSkeleton] = useState(false);

    // Reset pagination when channel changes
    useEffect(() => { setChDisplayCount(CH_PAGE_SIZE); }, [activeId]);

    // Load history from disk when switching to a channel with no in-memory messages
    useEffect(() => {
        if (!channelFileKey || !activeId) return;
        const key = channelKey;
        if ((allBridgeMessages[key] || []).length > 0) return; // already have messages in memory
        channelHistoryLoad(channelFileKey, 0, CH_PAGE_SIZE).then(result => {
            if (result.total > 0) setDiskTotalFor(channelFileKey, result.total);
            if (result.messages.length > 0) {
                setAllBridgeMessages(all => ({
                    ...all,
                    [key]: result.messages.map(m => ({ role: m.role, content: m.content })),
                }));
                setChDisplayCount(CH_PAGE_SIZE);
            }
        }).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [activeId, channelFileKey]);

    const handleChatScroll = () => {
        const container = chatContainerRef.current;
        if (!container) return;
        const isAtBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
        autoFollowRef.current = isAtBottom;
        setShowScrollBtn(!isAtBottom && messages.length > 0);

        if (container.scrollTop !== 0) return; // not at top, nothing to do

        // Phase 1: more in-memory messages to show
        if (chDisplayCount < messages.length) {
            setChShowSkeleton(true);
            const prevScrollHeight = container.scrollHeight;
            setTimeout(() => {
                setChShowSkeleton(false);
                setChDisplayCount(c => Math.min(c + CH_PAGE_SIZE, messages.length));
                requestAnimationFrame(() => {
                    if (chatContainerRef.current) {
                        chatContainerRef.current.scrollTop = chatContainerRef.current.scrollHeight - prevScrollHeight;
                    }
                });
            }, 300);
            return;
        }

        // Phase 2: load older batch from disk when in-memory is exhausted
        if (!channelFileKey) return;
        const total = getDiskTotal();
        const alreadyLoaded = messages.length; // how many we have in memory (= disk offset from end)
        if (alreadyLoaded >= total) return; // nothing older on disk

        setChShowSkeleton(true);
        const prevScrollHeight = container.scrollHeight;
        channelHistoryLoad(channelFileKey, alreadyLoaded, CH_PAGE_SIZE)
            .then(result => {
                setChShowSkeleton(false);
                if (result.messages.length === 0) return;
                const key = channelKey;
                const older = result.messages.map(m => ({ role: m.role, content: m.content }));
                setAllBridgeMessages(all => ({ ...all, [key]: [...older, ...(all[key] || [])] }));
                setChDisplayCount(c => c + result.messages.length);
                requestAnimationFrame(() => {
                    if (chatContainerRef.current) {
                        chatContainerRef.current.scrollTop = chatContainerRef.current.scrollHeight - prevScrollHeight;
                    }
                });
            })
            .catch(() => { setChShowSkeleton(false); });
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
            setDiskTotalFor(channelFileKey ?? '', 0);
            if (channelFileKey) channelHistoryClear(channelFileKey).catch(() => {});
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
                        return { id, name, github: skillData?.i || id, branch: skillData?.b || 'main' };
                    });
                    setSkillsFavorites(skills);
                } else {
                    setSkillsFavorites([]);
                }
            } catch { setSkillsFavorites([]); }
        }
        if (showSkillsPicker) setSkillsPage(0);
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
        if (!input.trim() && attachments.length === 0 && pendingModels.length === 0 && pendingSkills.length === 0) return;
        // Build message text + chips using shared utility
        const mdList = pendingModels.map(pm => {
            const md = modelList.find(m => m.internalId === pm.id);
            return {
                id: pm.id, name: pm.name, modelId: pm.modelId,
                baseUrl: md?.baseUrl, anthropicUrl: md?.anthropicUrl,
                apiKey: md?.apiKey, proxyUrl: md?.proxyUrl,
            };
        });
        const { messageText, chips } = buildPendingMessage(
            input,
            attachments.map((a, i) => ({ id: String(i), name: a.name, type: a.type as 'file' | 'image', preview: a.preview })),
            mdList,
            pendingSkills,
        );
        const text = messageText || input.trim();   // full text → Agent
        const displayText = input.trim();            // clean text → bubble & disk
        setPendingModels([]);
        setPendingSkills([]);
        setInput('');
        setAttachments([]);
        // Bubble shows clean user text + chips; agent receives full text with attachments
        setBridgeMessages(prev => [...prev, { role: 'user', content: displayText || '📎', chips } as any]);

        if (!canSendMessage) {
            // Blocked (e.g. still connecting) — show error so user knows, their message is still saved above
            setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.requestFailed' }]);
            return;
        }

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
                        setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: errorToKey(`Bridge start failed: ${startResult.error || 'Unknown error'}`) }]);
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
                    setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.noServerConfig' }]);
                    setBridgeLoading(false);
                    return;
                }
                setBridgeConnectionStatus('connected');
                setBridgeAgentName('OpenClaw'); // Default agent name for remote

                // After 30s, inject a "working" hint so the user knows the agent is running —
                // not frozen. It disappears when the real reply arrives.
                const WORKING_MARKER = '__agent_working__';
                const workingTimer = setTimeout(() => {
                    setBridgeMessages(prev => {
                        if (prev.some(m => m.content === WORKING_MARKER)) return prev;
                        return [...prev, { role: 'system', content: WORKING_MARKER }];
                    });
                }, 30_000);

                try {
                    const result = await api.bridgeChatRemote(serverId, text, bridgeSessionId);
                    clearTimeout(workingTimer);
                    // Remove the working hint before adding the real reply
                    setBridgeMessages(prev => {
                        const cleaned = prev.filter(m => m.content !== WORKING_MARKER);
                        return [...cleaned, {
                            role: 'assistant',
                            content: result.text,
                            meta: { model: result.model, tokens: result.tokens, duration_ms: result.duration_ms },
                        }];
                    });
                    if (result.session_id) setBridgeSessionId(result.session_id);
                } catch (remoteErr: any) {
                    clearTimeout(workingTimer);
                    setBridgeMessages(prev => {
                        const cleaned = prev.filter(m => m.content !== WORKING_MARKER);
                        return [...cleaned, { role: 'system', content: '', i18nKey: errorToKey(remoteErr?.message || String(remoteErr)) }];
                    });
                    return; // skip outer catch
                }
            }
        } catch (e: any) {
            setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: errorToKey(e?.message || String(e)) }]);
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
    }, [activeId, input, attachments, pendingModels, pendingSkills, isActiveConnected, canSendMessage, isLocalChannel, bridgeLoading, bridgeSessionId, bridgeConnectionStatus, activeChannel, modelList]);

    // Abort current request
    const handleAbort = useCallback(() => {
        if (!isBridgeMode) gateway.abort();
    }, [gateway, isBridgeMode]);

    return (
        <div className="flex h-full gap-0 overflow-hidden">
            {/* ======== Left: Channel list ======== */}
            <div className="w-56 flex-shrink-0 flex flex-col">
                <div className="flex-1 overflow-y-auto slim-scroll pr-2 py-2 space-y-2 custom-scrollbar">
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
                                <div className="flex items-center gap-2">
                                    {/* Left: status + name */}
                                    <div className="flex-1 min-w-0">
                                        {/* Status */}
                                        <div className="flex items-center gap-1.5 mb-1.5">
                                            <div className={`w-2 h-2 rounded-full ${isLinked ? 'bg-cyber-accent animate-pulse' : isBridgeConnecting ? 'bg-yellow-400 animate-pulse' : isError ? 'bg-red-400' : 'bg-cyber-text-muted/50'}`} />
                                            <span className={`text-xs tracking-wide ${isLinked ? 'text-cyber-accent' : isBridgeConnecting ? 'text-yellow-400' : isError ? 'text-red-400' : 'text-cyber-text-muted/70'}`}>
                                                [{isLinked ? t('channel.linked') : isBridgeConnecting ? t('channel.connecting') : isError ? t('channel.failed') : t('channel.standby')}]
                                            </span>
                                            {hasNew && (
                                                <span className="ml-auto w-2 h-2 rounded-full bg-red-500 animate-pulse" />
                                            )}
                                        </div>
                                        {/* Name */}
                                        <div className="flex items-center gap-1 h-5">
                                            <span className={`text-sm font-bold whitespace-nowrap truncate ${isActive ? 'text-cyber-accent' : 'text-cyber-accent/90'}`}>
                                                {ch.name || (ch.address?.startsWith('127.0.0.1') || ch.address === 'localhost'
                                                    ? `${t('mother.local')} (127.0.0.1)`
                                                    : ch.address)}
                                            </span>
                                        </div>
                                    </div>
                                    {/* Right: tool icon (synced with active agent tab) */}
                                    {(() => {
                                        const selectedAgent = allActiveAgents[ch.id] || 'OpenClaw';
                                        const agent = AGENT_LIST.find(a => a.name === selectedAgent) || AGENT_LIST[0];
                                        return (
                                            <img src={agent.icon} alt={agent.name} className="w-7 h-7 flex-shrink-0 opacity-70" />
                                        );
                                    })()}
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

                        {!isActiveConnected && !canSendMessage && gateway.status !== 'connecting' && bridgeConnectionStatus !== 'connecting' ? (
                            <div className="flex-1 mx-4 mt-2 bg-cyber-terminal rounded-lg flex items-center justify-center">
                                <div className="text-center font-mono space-y-3 select-none max-w-md">
                                    <p className="text-sm text-cyber-text-muted/50">
                                        {t('channel.motherFlow')}
                                    </p>
                                </div>
                            </div>
                        ) : (
                            /* Connected / Connecting — terminal chat area */
                            <div className="flex-1 flex flex-col mx-4 mt-2 min-h-0">
                                {/* Fixed agent tool tab bar */}
                                <div className="flex items-center gap-2 select-none py-2 flex-shrink-0">
                                    {AGENT_LIST.map(agent => {
                                        const selectedAgent = allActiveAgents[channelKey] || 'OpenClaw';
                                        const isActive = selectedAgent === agent.name;
                                        return (
                                            <div
                                                key={agent.name}
                                                onClick={() => setActiveAgentFor(channelKey, agent.name)}
                                                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-card text-xs font-mono cursor-pointer transition-all ${
                                                    isActive
                                                        ? 'border border-cyber-accent bg-cyber-accent/10 shadow-cyber-card text-cyber-accent'
                                                        : 'border border-cyber-border shadow-cyber-card bg-black/80 text-cyber-text-muted/70 hover:border-cyber-accent/30 hover:bg-black/90'
                                                }`}
                                            >
                                                <img src={agent.icon} alt={agent.name} className={`w-4 h-4 ${isActive ? '' : 'opacity-50 grayscale'}`} />
                                                <span>{agent.name}</span>
                                                <ArrowLeftRight size={10} className={isActive ? 'text-cyber-accent/60' : 'text-cyber-text-muted/30'} />
                                            </div>
                                        );
                                    })}
                                    {bridgeConnectionStatus === 'connecting' && (
                                        <span className="text-yellow-400 text-xs font-mono animate-pulse ml-2">Connecting...</span>
                                    )}
                                    {bridgeConnectionStatus === 'disconnected' && (
                                        <span className="text-red-400 text-xs font-mono ml-2">Connection failed</span>
                                    )}
                                </div>
                                {/* Scrollable chat area */}
                                <div className="relative flex-1 min-h-0">
                                <div ref={chatContainerRef} onScroll={handleChatScroll} className="absolute inset-0 overflow-y-auto slim-scroll custom-scrollbar p-4">
                                <div>

                                    {/* Bubble messages */}
                                    <div className="pt-2 pb-1">
                                    {chShowSkeleton && [0,1,2].map(i => (
                                        <ChatBubble key={`sk-${i}`} role="skeleton" content="" variant="channels" />
                                    ))}
                                    {messages.slice(-chDisplayCount).map((msg, i) => {
                                        // tool_call / result / thinking go to status bar only
                                        if (msg.role === 'tool_call' || msg.role === 'tool_result' || msg.role === 'thinking') return null;
                                        if (msg.role === 'system' && msg.content === '__agent_working__') return null;
                                        if (msg.role === 'user') return <ChatBubble key={i} role="user" content={msg.content} variant="channels" chips={msg.chips} />;
                                        if (msg.role === 'system') {
                                            const text = msg.i18nKey ? t(msg.i18nKey as import('../i18n/types').TKey) : msg.content;
                                            const isCancelled = msg.i18nKey === 'error.userCancelled';
                                            if (isCancelled)
                                                return <div key={i} className="flex justify-center my-1"><span className="text-cyber-text-muted/35 text-xs font-mono">{text}</span></div>;
                                            return <ChatBubble key={i} role="error" content={text} variant="channels" />;
                                        }
                                        return <ChatBubble key={i} role="assistant" content={msg.content} variant="channels" />;
                                    })}
                                    {bridgeLoading && <ChatBubble role="assistant" content="" variant="channels" isStreaming={true} />}
                                    </div>
                                    <div ref={scrollRef} />
                                </div>
                            </div>
                                {showScrollBtn && (
                                    <button
                                        onClick={scrollToBottom}
                                        className="absolute bottom-3 right-3 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-accent hover:border-cyber-accent/50 transition-colors z-10"
                                    ><svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 9l6 6 6-6" /></svg></button>
                                )}
                            </div>
                            </div>
                        )}

                        {/* Input area */}
<div className="flex-shrink-0 mx-4 mt-3 mb-2">
                            <div className="bg-cyber-terminal rounded-lg relative">
                                {/* Pending chips — shared component */}
                                <PendingChipsRow
                                    files={attachments.map((a, i) => ({ id: String(i), name: a.name, type: a.type as 'file'|'image', preview: a.preview }))}
                                    onRemoveFile={id => removeAttachment(Number(id))}
                                    models={pendingModels}
                                    onRemoveModel={id => setPendingModels(prev => prev.filter(m => m.id !== id))}
                                    skills={pendingSkills}
                                    onRemoveSkill={id => setPendingSkills(prev => prev.filter(s => s.id !== id))}
                                />
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
                                    rows={2}
                                    className="w-full bg-transparent px-4 py-2 text-sm text-[#DED9D2] font-sans font-medium outline-none placeholder:text-[#DED9D2]/40 disabled:opacity-30 resize-none"
                                />
                                {/* Bottom toolbar */}
                                <div className="flex items-center justify-between px-3 py-1.5">
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
                                            disabled={bridgeLoading || !isActiveConnected || pendingModels.length >= 5}
                                            className={`p-1 transition-colors disabled:opacity-20 ${pendingModels.length > 0 ? 'text-cyber-accent' : 'text-cyber-accent/60 hover:text-cyber-accent'}`}
                                        >
                                            <KeyRound size={15} />
                                        </button>
                                        <button
                                            onClick={openSkillsPicker}
                                            disabled={bridgeLoading || !isActiveConnected || pendingSkills.length >= 5}
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
                                                        <div>
                                                            {skillsFavorites.slice(skillsPage * SKILLS_PER_PAGE_CH, (skillsPage + 1) * SKILLS_PER_PAGE_CH).map(skill => (
                                                                <button
                                                                    key={skill.id}
                                                                    onClick={() => {
                                                                        setPendingSkills(prev => {
                                                                            if (prev.some(s => s.id === skill.id) || prev.length >= 5) return prev;
                                                                            return [...prev, { id: skill.id, name: skill.name, github: skill.github, branch: (skill as any).branch || 'main' }];
                                                                        });
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
                                                        {Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE_CH) > 1 && (
                                                            <div className="flex items-center justify-between px-3 py-1.5 border-t border-cyber-border/20 text-[10px] font-mono text-cyber-text-muted/50">
                                                                <button
                                                                    onClick={() => setSkillsPage(p => Math.max(0, p - 1))}
                                                                    disabled={skillsPage === 0}
                                                                    className="hover:text-cyber-warning disabled:opacity-30 transition-colors"
                                                                >
                                                                    <ChevronLeft size={12} />
                                                                </button>
                                                                <span>{skillsPage + 1} / {Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE_CH)}</span>
                                                                <button
                                                                    onClick={() => setSkillsPage(p => Math.min(Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE_CH) - 1, p + 1))}
                                                                    disabled={skillsPage >= Math.ceil(skillsFavorites.length / SKILLS_PER_PAGE_CH) - 1}
                                                                    className="hover:text-cyber-warning disabled:opacity-30 transition-colors"
                                                                >
                                                                    <ChevronRight size={12} />
                                                                </button>
                                                            </div>
                                                        )}
                                                    </>
                                                )}
                                            </div>
                                        )}
                                        {/* Model picker popover (upward) */}
                                        {showModelPicker && (
                                            <div
                                                ref={modelPickerRef}
                                                className="absolute bottom-full left-0 mb-2 w-64 max-h-48 overflow-y-auto slim-scroll bg-cyber-bg border border-cyber-border/60 rounded-lg shadow-lg z-50 custom-scrollbar"
                                            >
                                                {modelList.length === 0 ? (
                                                    <div className="px-3 py-2 text-xs text-cyber-text-muted/50 font-mono">{t('channel.noModels')}</div>
                                                ) : (
                                                    modelList.map(m => (
                                                        <button
                                                            key={m.internalId}
                                                            onClick={() => {
                                                                if (!pendingModels.some(pm => pm.id === m.internalId) && pendingModels.length < 5) {
                                                                    setPendingModels(prev => [...prev, { id: m.internalId, name: m.name, modelId: m.modelId }]);
                                                                }
                                                                setShowModelPicker(false);
                                                            }}
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
                                        {/* Clear display — frontend only, bridge/context unaffected */}
                                        <button
                                            onClick={() => {
                                                setBridgeMessages([]);
                                                setDiskTotalFor(channelFileKey ?? '', 0);
                                                if (channelFileKey) channelHistoryClear(channelFileKey).catch(() => {});
                                            }}
                                            disabled={messages.length === 0}
                                            className="p-1 text-cyber-accent/40 hover:text-cyber-accent transition-colors disabled:opacity-20"
                                        >
                                            <RotateCcw size={14} />
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
                                                disabled={(!input.trim() && attachments.length === 0 && pendingModels.length === 0 && pendingSkills.length === 0) || !isActiveConnected}
                                                className="w-6 h-6 rounded-lg flex items-center justify-center bg-cyber-accent hover:brightness-110 transition-all disabled:opacity-20"
                                            >
                                                <Send size={15} className="text-cyber-bg" />
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

        </div >
    );
};
