// Channels — OpenClaw agent chat interface (bridge CLI + SSH)
import React, { useState, useEffect, useRef, useCallback, createContext, useContext } from 'react';
import { Send, CornerDownLeft, X, Square, Paperclip, Image as ImageIcon, RotateCcw, Zap, Server, ChevronsDown, ChevronDown, ChevronLeft, ChevronRight } from 'lucide-react';
import { MiniSelect } from '../../components/MiniSelect';
import { getModelIcon } from '../../components/cards/ModelCard';
import { PendingChipsRow } from '../../components/PendingChipsRow';
import { RemoteModelSelector, type ModelOption } from '../../components/RemoteModelSelector';
import { AgentRolePicker } from '../../components/AgentRolePicker';
import { ChatBubble, TerminalStatusBar } from '../../components/chat';
import { MobileQRPopup } from '../../components/MobileQRPopup';

import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import { normalizeError, errorToKey } from '../../utils/normalizeError';
import { buildPendingMessage } from '../../utils/buildPendingMessage';
import type { ModelConfig } from '../../api/types';
import { useChatPersistence } from '../../hooks/useChatPersistence';
import type { DiskMsg } from '../../hooks/useChatPersistence';
import { useNavigationStore } from '../../stores/navigationStore';
import { AGENT_LIST } from '../../api/agentList';
import { ChannelsContext, useChannels } from './context';
import type { Channel, Attachment } from './context';


// ===== Provider =====
export function ChannelsProvider({ children }: { children: React.ReactNode }) {
    const [channels, setChannels] = useState<Channel[]>([]);
    const [activeId, setActiveId] = useState<number | null>(null);
    const [allBridgeStatus, setAllBridgeStatus] = useState<Record<number, string>>({});
    const [allActiveAgents, setAllActiveAgents] = useState<Record<number, string>>({});
    const [allBridgeLoading, setAllBridgeLoading] = useState<Record<number, boolean>>({});
    const [allSelectedRoles, setAllSelectedRoles] = useState<Record<number, { id: string; name: string; filePath: string }>>({});
    const [allBridgeHasNew, setAllBridgeHasNew] = useState<Record<number, boolean>>({});
    const selectChannel = useCallback((id: number) => {
        if (id !== activeId) setActiveId(id);
        setAllBridgeHasNew(prev => ({ ...prev, [id]: false }));
    }, [activeId]);
    return <ChannelsContext.Provider value={{ channels, setChannels, activeId, setActiveId, selectChannel, allBridgeStatus, setAllBridgeStatus, allActiveAgents, setAllActiveAgents, allBridgeLoading, setAllBridgeLoading, allSelectedRoles, setAllSelectedRoles, allBridgeHasNew, setAllBridgeHasNew }}>{children}</ChannelsContext.Provider>;
}

// Connection protocols (display as SSH since connections go through SSH port forwarding)
const PROTOCOLS = [
    { id: 'ws://', label: 'ssh://' },
    { id: 'wss://', label: 'ssh://' },
];

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

const ChannelsInner: React.FC = () => {
    const { t, locale } = useI18n();
    const { channels, setChannels, activeId, setActiveId, selectChannel, allBridgeStatus, setAllBridgeStatus, allActiveAgents, setAllActiveAgents, allBridgeLoading, setAllBridgeLoading } = useChannels();
    const [input, setInput] = useState('');
    const [arrowIndex, setArrowIndex] = useState(0);
    const [attachments, setAttachments] = useState<Attachment[]>([]);

    const [remoteCopied, setRemoteCopied] = useState('');

    // Process toggle (show/hide tool calls and thinking)

    // Bridge mode state — per-channel storage
    type BridgeMsg = { role: string; content: string; i18nKey?: string; meta?: { model?: string; tokens?: number; duration_ms?: number }; chips?: import('../../components/chat/ChatBubble').BubbleChip[] };
    const [allBridgeMessages, setAllBridgeMessages] = useState<Record<number, BridgeMsg[]>>({});
    const [allBridgeSessionIds, setAllBridgeSessionIds] = useState<Record<number, string>>({});
    const [allBridgeAgentNames, setAllBridgeAgentNames] = useState<Record<number, string>>({});
    // allBridgeLoading comes from context (ChannelsProvider)
    // Per-channel active agent selection
    const setActiveAgentFor = (chId: number, name: string) => {
        setAllActiveAgents(prev => ({ ...prev, [chId]: name }));
    };
    // Per-channel selected role
    const { allSelectedRoles, setAllSelectedRoles, setAllBridgeHasNew } = useChannels();
    const [showRolePicker, setShowRolePicker] = useState(false);
    // Track last applied role per channel to avoid redundant set_role calls
    const lastAppliedRoleRef = useRef<Record<number, string>>({});
    // Cache remote agent detection results per channel (avoid repeated SSH calls)
    const remoteAgentCache = useRef<Record<number, any[]>>({});

    // ── Remote model selector state (per-channel) ──
    const [allRemoteModels, setAllRemoteModels] = useState<Record<number, { id: string; name: string } | null>>({});
    const [allRemoteModelLoading, setAllRemoteModelLoading] = useState<Record<number, boolean>>({});
    const [channelModelList, setChannelModelList] = useState<ModelOption[]>([]);

    // Per-channel helpers
    const channelKey = activeId ?? 0;
    const bridgeMessages = allBridgeMessages[channelKey] || [];
    const bridgeSessionId = allBridgeSessionIds[channelKey];
    const bridgeConnectionStatus = allBridgeStatus[channelKey] || 'standby';
    const bridgeAgentName = allBridgeAgentNames[channelKey];
    const bridgeLoading = allBridgeLoading[channelKey] || false;
    const selectedRoleForChannel = allSelectedRoles[channelKey] || null;

    // ─── Persist agent/role selection per channel ───
    const channelFileKeyForPersist = channelKey === 0 ? null
        : channelKey === 1 ? 'local'
        : (channels.find(c => c.id === channelKey)?.address || `ch_${channelKey}`).replace(/[^a-zA-Z0-9._-]/g, '_');

    // Agent selection is persisted to localStorage keyed by channel address

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

    // ─── Chat persistence via shared hook ───
    const chToDisk = useCallback((m: BridgeMsg): DiskMsg | null => {
        if (m.role === 'user' || m.role === 'assistant') return { role: m.role, content: m.content };
        if (m.role === 'system') return { role: 'system', content: m.i18nKey || m.content };
        return null;
    }, []);
    const chFromDisk = useCallback((m: DiskMsg): BridgeMsg => {
        if (m.role === 'system' && m.content.startsWith('error.')) {
            return { role: 'system', content: '', i18nKey: m.content };
        }
        return { role: m.role, content: m.content };
    }, []);
    const chPrepend = useCallback((older: BridgeMsg[]) => {
        const key = channelKey;
        setAllBridgeMessages(all => ({ ...all, [key]: [...older, ...(all[key] || [])] }));
    }, [channelKey]);
    const chSetMessages = useCallback((msgs: BridgeMsg[]) => {
        const key = channelKey;
        setAllBridgeMessages(all => ({ ...all, [key]: msgs }));
    }, [channelKey]);
    const chPersistence = useChatPersistence<BridgeMsg>({
        diskKey: channelFileKey,
        messages: bridgeMessages,
        prependMessages: chPrepend,
        setMessages: chSetMessages,
        toDisk: chToDisk,
        fromDisk: chFromDisk,
    });

    const setBridgeLoading = (val: boolean) =>
        setAllBridgeLoading(all => ({ ...all, [channelKey]: val }));
    const setBridgeMessages = (updater: BridgeMsg[] | ((prev: BridgeMsg[]) => BridgeMsg[])) => {
        setAllBridgeMessages(all => {
            const next = typeof updater === 'function' ? updater(all[channelKey] || []) : updater;
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
    const sendingRef = useRef(false);
    const abortedRef = useRef(false);  // discard responses after user clicks Cancel
    const fileInputRef = useRef<HTMLInputElement>(null);
    const imageInputRef = useRef<HTMLInputElement>(null);



    const activeChannel = channels.find(c => c.id === activeId);
    // All channels use bridge mode (local = spawn, remote = SSH)
    const isBridgeMode = true;
    const isLocalChannel = activeId === 1;
    const isActiveConnected = bridgeConnectionStatus === 'connected' || bridgeConnectionStatus === 'standby';
    // Bridge standby/disconnected = allow sending (local auto-restarts, remote reconnects per-send via SSH)
    const remoteModelLoading = allRemoteModelLoading[channelKey] || false;
    const remoteModel = allRemoteModels[channelKey] || null;
    const canSendMessage = bridgeConnectionStatus !== 'connecting' && !remoteModelLoading;
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

            // Restore persisted agent selections from localStorage
            const restoredAgents: Record<number, string> = {};
            const restoredRoles: Record<number, { id: string; name: string; filePath: string }> = {};
            for (const ch of all) {
                const stableKey = ch.id === 1 ? 'local' : ch.address.replace(/[^a-zA-Z0-9._-]/g, '_');
                const savedAgent = localStorage.getItem(`ch_agent_${stableKey}`);
                if (savedAgent) restoredAgents[ch.id] = savedAgent;
                const savedRole = localStorage.getItem(`ch_role_${stableKey}`);
                if (savedRole) {
                    try { restoredRoles[ch.id] = JSON.parse(savedRole); } catch {}
                }
            }
            if (Object.keys(restoredAgents).length > 0) {
                setAllActiveAgents(prev => ({ ...prev, ...restoredAgents }));
            }
            if (Object.keys(restoredRoles).length > 0) {
                setAllSelectedRoles(prev => ({ ...prev, ...restoredRoles }));
            }
            // Model is restored separately after channelModelList loads (via useEffect below)
        } catch (e) {
            console.error('[Channels] Failed to load data:', e);
        }
    }, []);

    useEffect(() => {
        loadChannelData();
        // Refresh when SSH servers change (added/removed in Mother Agent)
        let prevVersion = useNavigationStore.getState().sshServersVersion;
        const unsub = useNavigationStore.subscribe((state) => {
            if (state.sshServersVersion !== prevVersion) {
                prevVersion = state.sshServersVersion;
                loadChannelData(true);
            }
        });
        return unsub;
    }, [loadChannelData]);

    // Note: channels.json polling removed — local channel uses bridge mode,
    // SSH channels are configured via server list

    // Save to config file on channel changes
    // Smart scroll: auto-follow unless user scrolls up
    const autoFollowRef = useRef(true);
    const isProgrammaticScrollRef = useRef(false);
    const [showScrollBtn, setShowScrollBtn] = useState(false);
    const chatContainerRef = useRef<HTMLDivElement>(null);

    // Reset pagination when channel changes
    useEffect(() => { chPersistence.resetDisplayCount(); }, [activeId]);

    // Load history from disk when switching to a channel with no in-memory messages
    // Then scroll to bottom after messages are loaded (fixes local channel starting at top)
    useEffect(() => {
        if (!channelFileKey || !activeId) return;
        // Always force auto-follow on channel switch so when messages arrive
        // (either from loadInitial or in-memory), the messages useEffect scrolls to bottom
        autoFollowRef.current = true;
        isProgrammaticScrollRef.current = false;
        if ((allBridgeMessages[channelKey] || []).length > 0) {
            // Already have in-memory messages — scroll immediately after render
            requestAnimationFrame(() => requestAnimationFrame(() => doScrollToBottom('auto')));
            return;
        }
        chPersistence.loadInitial();
        // No explicit scroll here — messages useEffect fires when loadInitial updates state,
        // and autoFollowRef=true ensures it scrolls to bottom at that point.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [activeId, channelFileKey]);

    const handleChatScroll = () => {
        const container = chatContainerRef.current;
        if (!container) return;
        // Skip autoFollow updates during programmatic scroll (scrollIntoView triggers scroll events)
        if (isProgrammaticScrollRef.current) return;
        const isAtBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 40;
        autoFollowRef.current = isAtBottom;
        setShowScrollBtn(!isAtBottom && messages.length > 0);
        chPersistence.handleScrollPagination(container);
    };

    // Helper: programmatic scroll that won't flip autoFollowRef
    const doScrollToBottom = (behavior: ScrollBehavior = 'auto') => {
        isProgrammaticScrollRef.current = true;
        autoFollowRef.current = true;
        setShowScrollBtn(false);
        const container = chatContainerRef.current;
        if (container) {
            if (behavior === 'smooth') {
                container.scrollTo({ top: container.scrollHeight, behavior: 'smooth' });
            } else {
                container.scrollTop = container.scrollHeight;
            }
        }
        // Reset flag after scroll events settle
        setTimeout(() => { isProgrammaticScrollRef.current = false; }, 150);
    };

    // Auto-scroll when messages change OR loading indicator appears/disappears
    // (The "Typing..." bubble changes DOM height — must scroll again to stay at bottom)
    // Double requestAnimationFrame ensures DOM layout is complete (especially for the typing bubble)
    useEffect(() => {
        if (autoFollowRef.current) {
            requestAnimationFrame(() => requestAnimationFrame(() => doScrollToBottom('auto')));
        }
    }, [messages, bridgeLoading]);

    // (Scroll on channel switch is now handled by the loadInitial useEffect above)

    const scrollToBottom = () => doScrollToBottom('smooth');

    // Loading animation
    useEffect(() => {
        if (!bridgeLoading) return;
        const timer = setInterval(() => {
            setArrowIndex(prev => (prev + 1) % 4);
        }, 200);
        return () => clearInterval(timer);
    }, [bridgeLoading]);

    // Listen for title bar role selector click
    useEffect(() => {
        const handler = () => setShowRolePicker(true);
        window.addEventListener('open-role-picker', handler);
        return () => window.removeEventListener('open-role-picker', handler);
    }, []);

    // Listen for clear-chat event from title bar
    useEffect(() => {
        const handler = () => { setBridgeMessages([]); chPersistence.clearHistory(); };
        window.addEventListener('clear-chat', handler);
        return () => window.removeEventListener('clear-chat', handler);
    }, [setBridgeMessages, chPersistence]);

    // Focus input on connect
    useEffect(() => {
        if (isActiveConnected) inputRef.current?.focus();
    }, [isActiveConnected]);





    // Full reset: disconnect + clear messages + clear address
    const handleReset = useCallback(async () => {
        if (!activeId) return;
        if (isBridgeMode) {
            setBridgeMessages([]);
            setBridgeSessionId(undefined);
            chPersistence.clearHistory();
            // Stop bridge process
            try {
                await api.bridgeStop();
            } catch (e) {
                console.error('[Channels] Failed to stop bridge:', e);
            }
            setBridgeConnectionStatus('standby');
        }
        setChannels(prev => prev.map(c =>
            c.id === activeId ? { ...c, address: '' } : c
        ));
    }, [activeId, isBridgeMode]);


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

    // ── Remote model: load model list + read current model when agent changes ──
    const selectedAgentForChannel = allActiveAgents[channelKey] || '';

    // Helper: re-read current model from config, map API model ID back to internalId
    const refreshCurrentModel = useCallback(() => {
        const agentEntry = AGENT_LIST.find(a => a.name === selectedAgentForChannel);
        if (!agentEntry) return;
        const readPromise = isLocalChannel
            ? api.bridgeGetLocalModel(agentEntry.id)
            : activeChannel?.serverId
                ? api.bridgeGetRemoteModel(String(activeChannel.serverId), agentEntry.id)
                : Promise.resolve(null);
        Promise.all([readPromise, api.getModels()])
            .then(([result, models]) => {
                if (result?.modelId) {
                    // Map API model ID back to internalId for selector match
                    const match = models.find(m => m.modelId === result.modelId || m.name === result.modelId);
                    const displayId = match?.internalId || result.modelId;
                    const displayName = match?.name || result.modelName || result.modelId;
                    setAllRemoteModels(prev => ({ ...prev, [channelKey]: { id: displayId, name: displayName } }));
                }
            })
            .catch(() => {});
    }, [selectedAgentForChannel, channelKey, isLocalChannel, activeChannel]);

    useEffect(() => {
        if (!selectedAgentForChannel) return;
        // Load available models from Model Nexus, filtered by agent protocol support
        api.getModels().then(models => {
            // Agents that only support Anthropic protocol
            const anthropicOnlyAgents = ['claudecode'];
            const agentEntry = AGENT_LIST.find((a: any) => a.name === selectedAgentForChannel);
            const isAnthropicOnly = agentEntry && anthropicOnlyAgents.includes(agentEntry.id);

            const filtered = isAnthropicOnly
                ? models.filter(m => !!m.anthropicUrl)  // Only models with anthropicUrl
                : models;                                // All models

            setChannelModelList(filtered.map(m => ({
                id: m.internalId,
                name: m.name,
                icon: getModelIcon(m.name, m.modelId),
            })));
        }).catch(() => {});

        // Read current model (local or remote), map API model ID back to internalId
        const agentEntry = AGENT_LIST.find(a => a.name === selectedAgentForChannel);
        if (!agentEntry) return;
        setAllRemoteModelLoading(prev => ({ ...prev, [channelKey]: true }));

        const readPromise = isLocalChannel
            ? api.bridgeGetLocalModel(agentEntry.id)
            : activeChannel?.serverId
                ? api.bridgeGetRemoteModel(String(activeChannel.serverId), agentEntry.id)
                : Promise.resolve(null);

        Promise.all([readPromise, api.getModels()])
            .then(([result, models]) => {
                if (result?.modelId) {
                    const match = models.find(m => m.modelId === result.modelId || m.name === result.modelId);
                    const displayId = match?.internalId || result.modelId;
                    const displayName = match?.name || result.modelName || result.modelId;
                    setAllRemoteModels(prev => ({ ...prev, [channelKey]: { id: displayId, name: displayName } }));
                } else {
                    setAllRemoteModels(prev => ({ ...prev, [channelKey]: null }));
                }
            })
            .catch(() => {
                setAllRemoteModels(prev => ({ ...prev, [channelKey]: null }));
            })
            .finally(() => {
                setAllRemoteModelLoading(prev => ({ ...prev, [channelKey]: false }));
            });
    }, [selectedAgentForChannel, channelKey, isLocalChannel]);

    // Re-load model list + current model when user returns to Channels page
    // (e.g. after adding/modifying models in Model Nexus or App Manager)
    useEffect(() => {
        if (!selectedAgentForChannel) return;
        let prevPage = useNavigationStore.getState().activePage;
        const unsub = useNavigationStore.subscribe((state) => {
            if (state.activePage !== prevPage) {
                prevPage = state.activePage;
                if (state.activePage !== 'channels') return;
                // Refresh model list
                api.getModels().then(models => {
                    const anthropicOnlyAgents = ['claudecode'];
                    const agentEntry = AGENT_LIST.find((a: any) => a.name === selectedAgentForChannel);
                    const isAnthropicOnly = agentEntry && anthropicOnlyAgents.includes(agentEntry.id);
                    const filtered = isAnthropicOnly
                        ? models.filter(m => !!m.anthropicUrl)
                        : models;
                    setChannelModelList(filtered.map(m => ({
                        id: m.internalId,
                        name: m.name,
                        icon: getModelIcon(m.name, m.modelId),
                    })));
                }).catch(() => {});
                // Refresh current model
                refreshCurrentModel();
            }
        });
        return unsub;
    }, [selectedAgentForChannel, refreshCurrentModel]);

    // Scroll to bottom when navigating back to Channels page from another page
    // Pages are always-mounted (hidden class), so activeId doesn't change — must listen to activePage
    useEffect(() => {
        let prevPage = useNavigationStore.getState().activePage;
        const unsub = useNavigationStore.subscribe((state) => {
            if (state.activePage !== prevPage) {
                prevPage = state.activePage;
                if (state.activePage !== 'channels') return;
                requestAnimationFrame(() => requestAnimationFrame(() => doScrollToBottom('auto')));
            }
        });
        return unsub;
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Handle remote model switch
    const handleModelSelect = useCallback(async (modelId: string) => {
        const agentEntry = AGENT_LIST.find(a => a.name === selectedAgentForChannel);
        if (!agentEntry) return;

        const previousModel = allRemoteModels[channelKey] || null;
        setAllRemoteModelLoading(prev => ({ ...prev, [channelKey]: true }));

        try {
            // Find full model config for API key + base URL
            const models = await api.getModels();
            const selected = models.find(m => m.internalId === modelId);
            if (!selected) throw new Error('Model not found');

            // Use API model ID (modelId) for the actual model identifier, not internalId
            const apiModelId = selected.modelId || selected.name;
            // Use correct base URL for the protocol
            // Anthropic-only agents (Claude Code): must use anthropicUrl
            // All other agents (OpenClaw, ZeroClaw, etc.): prefer baseUrl (OpenAI protocol)
            const anthropicOnlyAgents = ['claudecode'];
            const isAnthropicAgent = anthropicOnlyAgents.includes(agentEntry.id);
            const effectiveBaseUrl = isAnthropicAgent
                ? (selected.anthropicUrl || selected.baseUrl || '')
                : (selected.baseUrl || selected.anthropicUrl || '');
            const effectiveProtocol = isAnthropicAgent ? 'anthropic' : 'openai';

            if (isLocalChannel) {
                await api.bridgeSetLocalModel(
                    agentEntry.id,
                    apiModelId,
                    selected.name,
                    selected.apiKey || '',
                    effectiveBaseUrl,
                    effectiveProtocol,
                );
                // Restart persistent agents (stdio-json protocol) to clear memory and reload config
                // cli-oneshot agents (zeroclaw, picoclaw, claudecode) don't need restart — they read config fresh each message
                if (['nanobot', 'hermes'].includes(agentEntry.id)) {
                    try {
                        console.info('[Bridge] Restarting local agent to apply new model:', agentEntry.id);
                        await api.stopTool(agentEntry.id);
                        await api.startTool(agentEntry.id);
                    } catch (restartErr) {
                        console.warn('[Bridge] Agent restart failed (non-fatal):', restartErr);
                    }
                }
            } else {
                if (!activeChannel?.serverId) throw new Error('No server ID');
                await api.bridgeSetRemoteModel(
                    String(activeChannel.serverId),
                    agentEntry.id,
                    apiModelId,
                    selected.name,
                    selected.apiKey || '',
                    effectiveBaseUrl,
                    effectiveProtocol,
                );
                // Restart persistent agents (stdio-json protocol) — same logic as local
                if (['nanobot', 'hermes'].includes(agentEntry.id)) {
                    try {
                        console.info('[Bridge] Restarting remote agent to apply new model:', agentEntry.id);
                        await api.bridgeStopAgentRemote(String(activeChannel.serverId), agentEntry.id);
                        await api.bridgeStartAgentRemote(String(activeChannel.serverId), agentEntry.id);
                    } catch (restartErr) {
                        console.warn('[Bridge] Remote agent restart failed (non-fatal):', restartErr);
                    }
                }
            }
            setAllRemoteModels(prev => ({ ...prev, [channelKey]: { id: selected.internalId, name: selected.name } }));
            // Persist model selection to localStorage
            if (channelFileKeyForPersist) {
                localStorage.setItem(`ch_model_${channelFileKeyForPersist}`, JSON.stringify({ id: selected.internalId, name: selected.name }));
            }
            // Clear session so next chat starts fresh with new model config
            setBridgeSessionId(undefined);
        } catch (e) {
            // Rollback to previous model
            setAllRemoteModels(prev => ({ ...prev, [channelKey]: previousModel }));
            setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.requestFailed' }]);
        } finally {
            setAllRemoteModelLoading(prev => ({ ...prev, [channelKey]: false }));
        }
    }, [selectedAgentForChannel, channelKey, activeChannel, allRemoteModels, isLocalChannel, channelFileKeyForPersist]);

    // Restore model from localStorage after models finish loading (mirrors MobileApp pattern)
    // Also auto-applies the model to the bridge backend so local channel works on cold start
    useEffect(() => {
        if (!channelFileKeyForPersist || !channelModelList.length) return;
        const currentModel = allRemoteModels[channelKey];
        if (currentModel) return; // already have a model selected
        const savedModelJson = localStorage.getItem(`ch_model_${channelFileKeyForPersist}`);
        if (savedModelJson) {
            try {
                const saved = JSON.parse(savedModelJson);
                const match = channelModelList.find(m => m.id === saved.id);
                if (match) {
                    // Push model config to bridge backend (also sets in-memory state on success)
                    handleModelSelect(match.id);
                }
            } catch {}
        }
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [channelModelList.length, channelKey, channelFileKeyForPersist]);



    // Send message
    const handleSend = useCallback(async () => {
        if (!activeId) return;
        if (!canSendMessage) return;
        if (bridgeLoading) return;
        if (sendingRef.current) return; // Prevent concurrent sends (stale closure guard)
        if (!input.trim() && attachments.length === 0) return;
        sendingRef.current = true;
        abortedRef.current = false;
        // Build message text + chips using shared utility
        const { messageText, chips } = buildPendingMessage(
            input,
            attachments.map((a, i) => ({ id: String(i), name: a.name, type: a.type as 'file' | 'image', preview: a.preview })),
            [],
            [],
        );
        const text = messageText || input.trim();   // full text -> Agent
        const displayText = input.trim();            // clean text -> bubble & disk
        setInput('');
        setAttachments([]);
        // Ensure auto-follow is on so useEffect scrolls to bottom after render.
        // Also set isProgrammaticScrollRef to prevent layout-shift scroll events
        // (textarea shrinks → chat area grows → onScroll fires) from resetting autoFollow.
        autoFollowRef.current = true;
        isProgrammaticScrollRef.current = true;
        setTimeout(() => { isProgrammaticScrollRef.current = false; }, 300);
        setBridgeMessages(prev => [...prev, { role: 'user', content: displayText || '📎', chips } as any]);

        if (!canSendMessage) {
            // Blocked (e.g. still connecting) — show error so user knows, their message is still saved above
            setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.requestFailed' }]);
            return;
        }

        // Check if agent is selected
        const selectedAgent = allActiveAgents[channelKey] || '';
        if (!selectedAgent) {
            setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.agentFailed' }]);
            return;
        }

        // Check if model is selected (both local and remote channels)
        if (!remoteModel) {
            setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.noModelSelected' }]);
            return;
        }

        setBridgeLoading(true);
        try {
            if (isLocalChannel) {
                // Local channel: always call bridgeStart — backend handles:
                // - Same agent already running → returns immediately (no-op)
                // - Different agent → kills old bridge + starts new one
                // This ensures agent switches (e.g. OpenClaw → Claude Code) work correctly
                {
                    setBridgeConnectionStatus('connecting');
                    const selectedAgent = allActiveAgents[channelKey] || '';
                    const agentEntry = AGENT_LIST.find(a => a.name === selectedAgent);
                    const startResult = await api.bridgeStart(agentEntry?.id);
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

                // Set role if selected AND changed since last apply
                const role = selectedRoleForChannel;
                const lastApplied = lastAppliedRoleRef.current[channelKey];
                if (role?.filePath) {
                    const roleUrl = role.filePath;
                    const selectedAgent = allActiveAgents[channelKey] || '';
                    const agentEntry = AGENT_LIST.find(a => a.name === selectedAgent);
                    const agentId = agentEntry?.id || '';
                    const roleKey = `${agentId}:${role.id}`;

                    // Re-apply if role or agent changed
                    if (roleKey !== lastApplied) {
                        // Note: openclaw gateway restart is handled inside the Bridge binary
                        // (restart_gateway_if_needed → sends /new to reload SOUL.md).
                        // Do NOT call stopTool/startTool here — it would open a visible console window.
                        // Only restart non-bridge persistent agents (nanobot, hermes) that need
                        // process-level restart to pick up config changes.
                        if (['nanobot', 'hermes'].includes(agentId)) {
                            try {
                                console.info('[Bridge] Restarting local agent to apply new role:', agentId);
                                await api.stopTool(agentId);
                                await api.startTool(agentId);
                            } catch (e) {
                                console.warn('[Bridge] Failed to restart local agent (non-fatal):', e);
                            }
                        }

                        // Let errors propagate to outer catch — shows as send failure
                        await api.bridgeSetRoleLocal(agentId, role.id, roleUrl);

                        // Force new session so agent reads the updated role
                        setBridgeSessionId(undefined);
                        lastAppliedRoleRef.current[channelKey] = roleKey;
                    }
                } else if (lastApplied) {
                    // User cleared role → reset agent to default mode
                    const selectedAgent = allActiveAgents[channelKey] || '';
                    const agentEntry = AGENT_LIST.find(a => a.name === selectedAgent);
                    const agentId = agentEntry?.id || '';
                    try {
                        // Note: openclaw gateway restart is handled inside Bridge binary.
                        // Only restart non-bridge persistent agents that need process-level restart.
                        if (['nanobot', 'hermes'].includes(agentId)) {
                            console.info('[Bridge] Restarting local agent to clear role:', agentId);
                            await api.stopTool(agentId);
                            await api.startTool(agentId);
                        }
                        await api.bridgeSetRoleLocal(agentId, '', '');
                        setBridgeSessionId(undefined);
                        lastAppliedRoleRef.current[channelKey] = '';
                    } catch (e) {
                        console.warn('[Bridge] clear_role failed (non-fatal):', e);
                    }
                }

                const result = await api.bridgeChatLocal(text, bridgeSessionId, undefined, role?.name);
                if (abortedRef.current) return; // User cancelled — discard response
                if (result.session_id) setBridgeSessionId(result.session_id);
                if (!result.text || result.text.trim() === '') {
                    // Empty response from agent — show error instead of invisible bubble
                    useNavigationStore.getState().incrementFlashCount();
                    setBridgeMessages(prev => [...prev, { role: 'system', content: 'Agent returned an empty response. The agent process may have crashed — try sending again.' }]);
                } else {
                    setBridgeMessages(prev => [...prev, {
                        role: 'assistant',
                        content: result.text,
                        meta: { model: result.model, tokens: result.tokens, duration_ms: result.duration_ms },
                    }]);
                }
                // Mark channel as having new message (for red dot when user is on another channel)
                setAllBridgeHasNew(prev => ({ ...prev, [channelKey]: true }));
            } else {
                // Remote channel: SSH → bridge binary on remote server
                const serverId = activeChannel?.serverId;
                if (!serverId) {
                    setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.noServerConfig' }]);
                    setBridgeLoading(false);
                    return;
                }
                // Don't set 'connected' yet — wait until Step 4 succeeds
                setBridgeConnectionStatus('connecting');
                const selectedAgentName = allActiveAgents[channelKey] || '';
                setBridgeAgentName(selectedAgentName);

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
                    const agentEntry = AGENT_LIST.find(a => a.name === selectedAgentName);
                    const agentId = agentEntry?.id || '';

                    // ── Step 1: Detect if agent is installed on remote server ──
                    // (cached per channel — only detect once, not every message)
                    if (!remoteAgentCache.current[channelKey]) {
                        // Ensure bridge binary exists on remote first (3-layer verification)
                        await api.bridgeEnsureRemote(serverId);
                        const agents = await api.bridgeDetectAgentsRemote(serverId);
                        remoteAgentCache.current[channelKey] = agents;
                    }
                    const cachedAgents = remoteAgentCache.current[channelKey];
                    const agentInfo = cachedAgents?.find((a: any) => a.id === agentId);
                    if (!agentInfo?.installed) {
                        setBridgeConnectionStatus('standby');
                        setBridgeMessages(prev => [...prev, {
                            role: 'system',
                            content: `Agent "${selectedAgentName}" is not installed on this server.`,
                        }]);
                        setBridgeLoading(false);
                        clearTimeout(workingTimer);
                        return;
                    }

                    // ── Step 2: Start agent if not running ──
                    if (!agentInfo.running) {
                        try {
                            await api.bridgeStartAgentRemote(serverId, agentId);
                            // Update cache: mark as running
                            agentInfo.running = true;
                        } catch (e) {
                            console.warn('[Bridge] start_agent failed (non-fatal):', e);
                        }
                    }

                    // ── Step 3: Set role if selected ──
                    const role = selectedRoleForChannel;
                    const lastApplied = lastAppliedRoleRef.current[channelKey];
                    if (role?.filePath) {
                        const roleUrl = role.filePath;
                        const roleKey = `${agentId}:${role.id}`;
                        if (roleKey !== lastApplied) {
                            try {
                                // Restart persistent agents to clear memory before role change
                                // openclaw: reads SOUL.md only at session start — must stop process so bridge restarts it fresh
                                if (['openclaw'].includes(agentId)) {
                                    console.info('[Bridge] Restarting remote agent to apply new role:', agentId);
                                    await api.bridgeStopAgentRemote(serverId, agentId);
                                    // Note: bridge auto-restarts openclaw on next chat message, no explicit start_agent needed
                                }
                                await api.bridgeSetRoleRemote(serverId, agentId, role.id, roleUrl);
                                lastAppliedRoleRef.current[channelKey] = roleKey;
                                // Force new session so agent reads updated role file
                                // (OpenClaw only reads SOUL.md at session start)
                                setBridgeSessionId(undefined);
                            } catch (e) {
                                console.warn('[Bridge] set_role failed (non-fatal):', e);
                            }
                        }
                    } else if (lastApplied) {
                        // User cleared role → reset agent to default mode
                        // Pass actual previous role_id (not empty string)
                        const lastRoleId = lastApplied.split(':')[1] || '';
                        try {
                            // Restart persistent agents to clear memory before clearing role
                            if (['openclaw'].includes(agentId)) {
                                console.info('[Bridge] Restarting remote agent to clear role:', agentId);
                                await api.bridgeStopAgentRemote(serverId, agentId);
                                // Note: bridge auto-restarts openclaw on next chat message, no explicit start_agent needed
                            }
                            await api.bridgeClearRoleRemote(serverId, agentId, lastRoleId);
                            setBridgeSessionId(undefined);
                            lastAppliedRoleRef.current[channelKey] = '';
                        } catch (e) {
                            console.warn('[Bridge] clear_role failed (non-fatal):', e);
                        }
                    }

                    // ── Step 4: Send message to agent ──
                    const result = await api.bridgeChatRemote(serverId, text, bridgeSessionId, agentId, role?.name);
                    clearTimeout(workingTimer);
                    // If user cancelled while waiting, discard the response
                    if (abortedRef.current) {
                        setBridgeMessages(prev => prev.filter(m => m.content !== WORKING_MARKER));
                        return;
                    }
                    // Success → now mark as connected
                    setBridgeConnectionStatus('connected');
                    // Remove the working hint before adding the real reply
                    if (!result.text || result.text.trim() === '') {
                        // Empty response from agent — show error instead of invisible bubble
                        useNavigationStore.getState().incrementFlashCount();
                        setBridgeMessages(prev => {
                            const cleaned = prev.filter(m => m.content !== WORKING_MARKER);
                            return [...cleaned, { role: 'system', content: 'Agent returned an empty response. The agent process may have crashed — try sending again.' }];
                        });
                    } else {
                        setBridgeMessages(prev => {
                            const cleaned = prev.filter(m => m.content !== WORKING_MARKER);
                            return [...cleaned, {
                                role: 'assistant',
                                content: result.text,
                                meta: { model: result.model, tokens: result.tokens, duration_ms: result.duration_ms },
                            }];
                        });
                    }
                    if (result.session_id) setBridgeSessionId(result.session_id);
                    setAllBridgeHasNew(prev => ({ ...prev, [channelKey]: true }));
                } catch (remoteErr: any) {
                    clearTimeout(workingTimer);
                    // Error → reset all transient state back to initial
                    setBridgeConnectionStatus('standby');
                    setBridgeAgentName(undefined);
                    useNavigationStore.getState().incrementFlashCount();
                    setBridgeMessages(prev => {
                        const cleaned = prev.filter(m => m.content !== WORKING_MARKER);
                        return [...cleaned, { role: 'system', content: '', i18nKey: errorToKey(remoteErr?.message || String(remoteErr)) }];
                    });
                    return; // skip outer catch
                }
            }
        } catch (e: any) {
            useNavigationStore.getState().incrementFlashCount();
            setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: errorToKey(e?.message || String(e)) }]);
            if (isLocalChannel) {
                try {
                    const s = await api.bridgeStatus();
                    setBridgeConnectionStatus(s.status || 'disconnected');
                } catch { setBridgeConnectionStatus('disconnected'); }
            }
        } finally {
            setBridgeLoading(false);
            sendingRef.current = false;
        }
        inputRef.current?.focus();
    }, [activeId, input, attachments, isActiveConnected, canSendMessage, isLocalChannel, bridgeLoading, bridgeSessionId, bridgeConnectionStatus, activeChannel, allActiveAgents, channelKey]);

    // Abort current request — stops loading and discards late-arriving response
    const handleAbort = useCallback(() => {
        abortedRef.current = true;
        setBridgeLoading(false);
        sendingRef.current = false;
        setBridgeMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.userCancelled' }]);
    }, []);

    return (
        <>
        <div className="flex flex-col h-full">
            {/* Chat area wrapper — matches Mother Agent layout exactly */}
            <div className="relative flex-1">
                <div ref={chatContainerRef} onScroll={handleChatScroll} className="absolute inset-0 overflow-y-auto slim-scroll p-4">
                    <div className="pt-2 pb-8">
                    {chPersistence.showSkeleton && [0,1,2].map(i => (
                        <ChatBubble key={`sk-${i}`} role="skeleton" content="" variant="channels" />
                    ))}
                    {activeChannel && messages.slice(-chPersistence.displayCount).map((msg, i, arr) => {
                        if (msg.role === 'system' && msg.content === '__agent_working__') return null;
                        if (msg.role === 'user') return <ChatBubble key={i} role="user" content={msg.content} variant="channels" chips={msg.chips} />;
                        if (msg.role === 'system') {
                            const text = msg.i18nKey ? t(msg.i18nKey as import('../../i18n/types').TKey) : msg.content;
                            const isCancelled = msg.i18nKey === 'error.userCancelled';
                            if (isCancelled)
                                return <div key={i} className="flex justify-center my-4"><span className="text-cyber-text-muted/35 text-xs font-mono">{text}</span></div>;
                            return <ChatBubble key={i} role="error" content={text} variant="channels" />;
                        }
                        return <ChatBubble key={i} role="assistant" content={msg.content} variant="channels" />;
                    })}
                    {bridgeLoading && <ChatBubble role="assistant" content="" variant="channels" isStreaming={true} />}
                    <div ref={scrollRef} style={{ height: 1 }} />
                    </div>
                </div>
                {/* Scroll-to-bottom button — bottom-right of chat area, like Mother Agent */}
                {showScrollBtn && (
                    <button onClick={scrollToBottom} className="absolute bottom-3 right-4 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-accent hover:border-cyber-accent/50 transition-colors z-10">
                        <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 9l6 6 6-6" /></svg>
                    </button>
                )}
            </div>

            {/* Input area */}
            {activeChannel && (
                <div className="flex-shrink-0 mt-1 mb-1">
                    <div className="bg-cyber-input rounded-lg p-2">
                        <PendingChipsRow
                            files={attachments.map((a, i) => ({ id: String(i), name: a.name, type: a.type as 'file'|'image', preview: a.preview }))}
                            onRemoveFile={id => removeAttachment(Number(id))}
                        />
                        <textarea
                            ref={inputRef}
                            value={input}
                            onChange={e => setInput(e.target.value)}
                            onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
                            onPaste={handlePaste}
                            placeholder={bridgeLoading ? t('channel.awaitingResponse') : t('channel.enterMessage')}
                            disabled={bridgeLoading || (!canSendMessage && !isActiveConnected)}
                            rows={2}
                            className="w-full bg-transparent px-2 py-1 text-sm text-[#DED9D2] font-sans font-medium outline-none placeholder:text-[#DED9D2]/40 disabled:opacity-30 resize-none"
                        />
                        <div className="flex items-center justify-between gap-1.5">
                                {/* Left: role/agent selector — matches RemoteModelSelector style */}
                                {(() => {
                                    const agentObj = selectedAgentForChannel ? AGENT_LIST.find(a => a.name === selectedAgentForChannel) : null;
                                    const label = selectedRoleForChannel?.id
                                        ? selectedRoleForChannel.name
                                        : (selectedAgentForChannel || t('channel.selectRoleAgent'));
                                    return (
                                        <button
                                            type="button"
                                            onClick={() => window.dispatchEvent(new CustomEvent('open-role-picker'))}
                                            className="flex items-center gap-1.5 px-2 py-1 text-xs font-mono text-cyber-accent transition-colors rounded hover:bg-white/8 active:bg-white/12 cursor-pointer min-w-0 max-w-[45%]"
                                        >
                                            {agentObj?.icon && (
                                                <img src={agentObj.icon} alt="" className="w-3.5 h-3.5 flex-shrink-0" onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }} />
                                            )}
                                            <span className="truncate">{label}</span>
                                            <ChevronDown size={11} className="flex-shrink-0 opacity-60" />
                                        </button>
                                    );
                                })()}
                                {/* Right: model selector + send/abort */}
                                <div className="flex items-center gap-1.5 flex-shrink-0">
                            {selectedAgentForChannel && (
                                <RemoteModelSelector
                                    models={channelModelList}
                                    currentModelId={remoteModel?.id || null}
                                    loading={remoteModelLoading}
                                    onSelect={handleModelSelect}
                                    placeholder={t('mother.selectModel')}
                                />
                            )}
                            {bridgeLoading ? (
                                <button onClick={handleAbort} className="w-8 h-8 rounded-lg flex items-center justify-center bg-red-500/20 hover:bg-red-500/30 transition-colors"><Square size={14} fill="#f87171" className="text-red-400" /></button>
                            ) : (
                                <button onClick={handleSend} disabled={(!input.trim() && attachments.length === 0) || !isActiveConnected || remoteModelLoading} className="w-8 h-8 rounded-lg flex items-center justify-center bg-cyber-accent hover:brightness-110 transition-all disabled:opacity-20"><Send size={18} className="text-cyber-bg rotate-45 -translate-x-[1px]" /></button>
                            )}
                                </div>
                        </div>
                    </div>
                </div>
            )}

            <AgentRolePicker
                isOpen={showRolePicker}
                onClose={() => setShowRolePicker(false)}
                selectedRole={selectedRoleForChannel?.id || null}
                onSelectRole={(id, name, filePath) => {
                    setAllSelectedRoles(prev => ({ ...prev, [channelKey]: { id, name, filePath } }));
                    // Persist role selection to localStorage
                    if (channelFileKeyForPersist) {
                        if (id) {
                            localStorage.setItem(`ch_role_${channelFileKeyForPersist}`, JSON.stringify({ id, name, filePath }));
                        } else {
                            localStorage.removeItem(`ch_role_${channelFileKeyForPersist}`);
                        }
                    }
                }}
                selectedAgent={allActiveAgents[channelKey] || ''}
                onSelectAgent={(name) => {
                    const previousAgent = allActiveAgents[channelKey] || '';
                    setActiveAgentFor(channelKey, name);
                    // Persist agent selection to localStorage
                    if (channelFileKeyForPersist) {
                        if (name) {
                            localStorage.setItem(`ch_agent_${channelFileKeyForPersist}`, name);
                        } else {
                            localStorage.removeItem(`ch_agent_${channelFileKeyForPersist}`);
                        }
                    }
                    // Any agent change (switch or clear) → full reset to initial state
                    if (previousAgent !== name) {
                        // Clear remote model + model list (avoids stale flash)
                        setAllRemoteModels(prev => ({ ...prev, [channelKey]: null }));
                        setChannelModelList([]);
                        // Reset bridge state
                        setBridgeConnectionStatus('standby');
                        setBridgeSessionId(undefined);
                        setBridgeAgentName(undefined);
                        lastAppliedRoleRef.current[channelKey] = '';
                        // Clear role when switching agents (roles are agent-specific)
                        setAllSelectedRoles(prev => { const next = { ...prev }; delete next[channelKey]; return next; });
                        if (channelFileKeyForPersist) localStorage.removeItem(`ch_role_${channelFileKeyForPersist}`);
                        // Clear model when switching agents
                        if (channelFileKeyForPersist) localStorage.removeItem(`ch_model_${channelFileKeyForPersist}`);
                        // Invalidate remote agent detection cache
                        delete remoteAgentCache.current[channelKey];
                    }
                    // Agent switched (not just cleared) → stop old process
                    if (previousAgent && previousAgent !== name) {
                        api.bridgeStop().catch(() => {});
                    }
                }}
                isRemote={!isLocalChannel}
                remoteServerId={activeChannel?.serverId}
            />
        </div>
        </>
    );
};

// ===== Exports =====
export { ChannelsInner as ChannelsMain };

