// MobileApp.tsx — Mobile-only application shell (vertical Channels page)
// Uses the SAME Tauri APIs and config files as PC Channels page.
// Flow: Server List (from loadSSHServers) → Chat (Telegram layout)
// Setup: tap header → Agent detection (bridgeDetectAgentsRemote) + Role list (scanRoles CDN)
// Model: bottom bar selector (bridgeGetRemoteModel / getModels)

import { useState, useEffect, useRef, useCallback } from 'react';
declare const __APP_VERSION__: string;
import { ChevronLeft, Settings, Send, Loader2, Box, Ban, Check, Globe, Download, ExternalLink, ChevronDown, ClipboardPaste, X } from 'lucide-react';
import { useI18n } from '../hooks/useI18n';
import { errorToKey } from '../utils/normalizeError';
import { useChatPersistence } from '../hooks/useChatPersistence';
import type { DiskMsg } from '../hooks/useChatPersistence';
import * as api from '../api/tauri';
import type { SSHServer, RemoteAgentInfo, RoleEntry, RoleCategory } from '../api/tauri';
import { getModelIcon } from '../components/cards/ModelCard';
import type { ModelConfig } from '../api/types';
import './MobileApp.css';
import OverscrollWrap from './OverscrollWrap';

type MobileScreen = 'servers' | 'chat' | 'setup' | 'settings';

// Agent list — same as PC Channels.tsx AGENT_LIST (icons bundled in app)
const AGENT_LIST = [
    { id: 'openclaw', name: 'OpenClaw', icon: '/icons/tools/openclaw.svg' },
    { id: 'claudecode', name: 'Claude Code', icon: '/icons/tools/claudecode.svg' },
    { id: 'zeroclaw', name: 'ZeroClaw', icon: '/icons/tools/zeroclaw.png' },
    { id: 'nanobot', name: 'NanoBot', icon: '/icons/tools/nanobot.png' },
    { id: 'picoclaw', name: 'PicoClaw', icon: '/icons/tools/picoclaw.png' },
    { id: 'hermes', name: 'Hermes Agent', icon: '/icons/tools/hermes.png' },
];

// Detect if running inside Tauri (real device) or browser dev mode
const hasTauri = () => !!(window as any).__TAURI_INTERNALS__;

// All supported locales (same as PC SettingsDialog)
const LOCALE_OPTIONS = [
    { id: 'en', label: 'English' },
    { id: 'zh-Hans', label: '简体中文' },
    { id: 'zh-Hant', label: '繁體中文' },
    { id: 'ja', label: '日本語' },
    { id: 'ko', label: '한국어' },
    { id: 'de', label: 'Deutsch' },
    { id: 'fr', label: 'Français' },
    { id: 'es', label: 'Español' },
    { id: 'pt', label: 'Português' },
    { id: 'it', label: 'Italiano' },
    { id: 'nl', label: 'Nederlands' },
    { id: 'ru', label: 'Русский' },
    { id: 'ar', label: 'العربية' },
    { id: 'hi', label: 'हिन्दी' },
    { id: 'bn', label: 'বাংলা' },
    { id: 'th', label: 'ไทย' },
    { id: 'vi', label: 'Tiếng Việt' },
    { id: 'id', label: 'Bahasa Indonesia' },
    { id: 'ms', label: 'Bahasa Melayu' },
    { id: 'tr', label: 'Türkçe' },
    { id: 'pl', label: 'Polski' },
    { id: 'cs', label: 'Čeština' },
    { id: 'hu', label: 'Magyar' },
    { id: 'sv', label: 'Svenska' },
    { id: 'fi', label: 'Suomi' },
    { id: 'el', label: 'Ελληνικά' },
    { id: 'he', label: 'עברית' },
    { id: 'fa', label: 'فارسی' },
];

/** Returns true only if `remote` version is strictly greater than `local` (semver X.Y.Z) */
function isNewerVersion(remote: string, local: string): boolean {
    const parse = (v: string) => v.replace(/^v/, '').split('.').map(Number);
    const [rMaj, rMin, rPat] = parse(remote);
    const [lMaj, lMin, lPat] = parse(local);
    if (rMaj !== lMaj) return rMaj > lMaj;
    if (rMin !== lMin) return rMin > lMin;
    return rPat > lPat;
}

// Mock data for browser dev testing (when Tauri backend unavailable)
const MOCK_SERVERS: SSHServer[] = [
    { id: 'mock_a100', host: '10.0.5.12', port: 22, username: 'admin', password: 'enc:v1:mock', alias: 'A100-Cluster' },
    { id: 'mock_4090', host: '172.16.0.88', port: 22, username: 'dev', password: 'enc:v1:mock', alias: 'RTX4090-Lab' },
    { id: 'mock_mac', host: '192.168.1.200', port: 22, username: 'coder', password: 'enc:v1:mock', alias: 'Mac-Studio' },
    { id: 'mock_cloud', host: '43.128.77.15', port: 22, username: 'ubuntu', password: 'enc:v1:mock', alias: 'Cloud-GPU-01' },
    { id: 'mock_home', host: '192.168.10.39', port: 22, username: 'eben', password: 'enc:v1:mock', alias: 'Home-Server' },
    { id: 'mock_edge', host: '10.10.1.5', port: 2222, username: 'root', password: 'enc:v1:mock', alias: 'Edge-Node' },
];

const MOCK_AGENTS: Record<string, RemoteAgentInfo> = {
    openclaw: { id: 'openclaw', name: 'OpenClaw', installed: true, running: false },
    claudecode: { id: 'claudecode', name: 'Claude Code', installed: false, running: false },
    zeroclaw: { id: 'zeroclaw', name: 'ZeroClaw', installed: true, running: false },
    nanobot: { id: 'nanobot', name: 'NanoBot', installed: false, running: false },
    picoclaw: { id: 'picoclaw', name: 'PicoClaw', installed: false, running: false },
    hermes: { id: 'hermes', name: 'Hermes Agent', installed: true, running: false },
};

const MOCK_MODELS: ModelConfig[] = [
    { internalId: 'mock-1', name: 'GPT-4o', baseUrl: 'https://api.openai.com/v1', apiKey: '' },
    { internalId: 'mock-2', name: 'Claude 3.5 Sonnet', baseUrl: 'https://api.anthropic.com', apiKey: '', anthropicUrl: 'https://api.anthropic.com' },
    { internalId: 'mock-3', name: 'DeepSeek-V3', baseUrl: 'https://api.deepseek.com/v1', apiKey: '' },
    { internalId: 'mock-4', name: 'Qwen2.5-72B', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', apiKey: '' },
    { internalId: 'mock-5', name: 'Gemini 2.0 Flash', baseUrl: 'https://generativelanguage.googleapis.com/v1beta', apiKey: '' },
    { internalId: 'mock-6', name: 'Llama-3.3-70B (Local)', baseUrl: 'http://127.0.0.1:8080/v1', apiKey: '' },
];

interface ChatMsg {
    role: 'user' | 'ai' | 'system';
    content: string;
    i18nKey?: string;
    model?: string;
    tokens?: number;
    duration_ms?: number;
}


// Settings screen component — language, version check, website link
interface SettingsScreenProps {
    locale: string;
    setLocale: (l: string) => void;
    onBack: () => void;
    t: (key: any) => string;
    slideClass?: string;
}
function SettingsScreen({ locale, setLocale, onBack, t, slideClass }: SettingsScreenProps) {
    const [updateStatus, setUpdateStatus] = useState<'idle' | 'checking' | 'latest' | 'available' | 'error'>('idle');
    const [latestVersion, setLatestVersion] = useState<string | null>(null);
    const [langOpen, setLangOpen] = useState(false);
    const appVersion = typeof __APP_VERSION__ !== 'undefined' ? __APP_VERSION__ : 'dev';

    const checkForUpdates = useCallback(async () => {
        setUpdateStatus('checking');
        try {
            const res = await fetch('https://echobird.ai/api/version/index.json');
            if (!res.ok) { setUpdateStatus('error'); return; }
            const data = await res.json();
            if (data.version && isNewerVersion(data.version, appVersion)) {
                setLatestVersion(data.version);
                setUpdateStatus('available');
            } else {
                setUpdateStatus('latest');
            }
        } catch {
            setUpdateStatus('error');
        }
    }, [appVersion]);

    const currentLang = LOCALE_OPTIONS.find(o => o.id === locale)?.label || 'English';

    return (
        <div className={`mobile-screen ${slideClass || ''}`}>
            <div className="mobile-header">
                <button className="mobile-icon-btn" onClick={onBack}>
                    <ChevronLeft size={22} />
                </button>
                <h2 className="mobile-title">{t('settings.title')}</h2>
                <div className="mobile-header-spacer" />
            </div>
            <div className="settings-list">
                {/* Version */}
                <div className="settings-item">
                    <span>{t('settings.version')}</span>
                    <span className="settings-value">v{appVersion}</span>
                </div>

                {/* Language */}
                <div className="settings-section">
                    <div className="settings-section-label">
                        <Globe size={14} />
                        <span>{t('settings.language')}</span>
                    </div>
                    <div className="settings-lang-select" onClick={() => setLangOpen(!langOpen)}>
                        <span>{currentLang}</span>
                        <ChevronDown size={14} className={langOpen ? 'rotate-180' : ''} />
                    </div>
                    {langOpen && (
                        <div className="settings-lang-dropdown">
                            {LOCALE_OPTIONS.map(opt => (
                                <div
                                    key={opt.id}
                                    className={`settings-lang-option ${opt.id === locale ? 'active' : ''}`}
                                    onClick={() => { setLocale(opt.id); setLangOpen(false); }}
                                >
                                    {opt.label}
                                </div>
                            ))}
                        </div>
                    )}
                </div>

                <div className="settings-divider" />

                {/* Update check */}
                <div className="settings-section">
                    <div className="settings-section-label">
                        <Download size={14} />
                        <span>{t('settings.updates')}</span>
                    </div>
                    {updateStatus === 'idle' && (
                        <button className="settings-update-btn" onClick={checkForUpdates}>
                            {t('settings.checkForUpdates')}
                        </button>
                    )}
                    {updateStatus === 'checking' && (
                        <div className="settings-update-status checking">
                            <Loader2 size={14} className="spin" /> {t('settings.checking')}
                        </div>
                    )}
                    {updateStatus === 'latest' && (
                        <div className="settings-update-status latest">
                            ✓ {t('settings.latestVersion')}
                        </div>
                    )}
                    {updateStatus === 'available' && (
                        <button className="settings-update-btn available" onClick={() => hasTauri() && api.openExternal('https://echobird.ai/')}>
                            UPDATE TO v{latestVersion} <ExternalLink size={10} />
                        </button>
                    )}
                    {updateStatus === 'error' && (
                        <button className="settings-update-btn error" onClick={checkForUpdates}>
                            {t('settings.checkFailed')}
                        </button>
                    )}
                </div>

                {/* Website */}
                <div className="settings-website">
                    <span onClick={() => hasTauri() && api.openExternal('https://echobird.ai')}>
                        EchoBird.ai <ExternalLink size={11} />
                    </span>
                </div>
            </div>
        </div>
    );
}


function MobileApp() {
    const { t, locale, setLocale } = useI18n();
    const [screen, setScreen] = useState<MobileScreen>('servers');
    const [slideDirection, setSlideDirection] = useState<'slide-right' | 'slide-left' | ''>('');

    // ── Fix keyboard covering input: use visualViewport height ──
    useEffect(() => {
        const vv = window.visualViewport;
        if (!vv) return;
        let prevHeight = vv.height;
        const update = () => {
            document.documentElement.style.setProperty('--app-height', `${vv.height}px`);
            // Keyboard opened (height shrank) → scroll chat to bottom
            if (vv.height < prevHeight - 50 && chatBottomRef.current) {
                setTimeout(() => chatBottomRef.current?.scrollIntoView({ behavior: 'smooth' }), 100);
            }
            prevHeight = vv.height;
        };
        update();
        vv.addEventListener('resize', update);
        return () => vv.removeEventListener('resize', update);
    }, []);

    // Screen depth for determining slide direction
    const screenDepth: Record<MobileScreen, number> = { settings: 0, servers: 1, chat: 2, setup: 3 };
    const navigateTo = useCallback((target: MobileScreen) => {
        const dir = screenDepth[target] >= screenDepth[screen] ? 'slide-right' : 'slide-left';
        setSlideDirection(dir);
        setScreen(target);
        // Clear animation class after it completes so re-triggering works
        setTimeout(() => setSlideDirection(''), 300);
    }, [screen]);

    // Server list — loaded from same config as PC (ssh_servers.json)
    const [servers, setServers] = useState<SSHServer[]>([]);
    const [serversLoading, setServersLoading] = useState(true);
    const [activeServer, setActiveServer] = useState<SSHServer | null>(null);

    // Agent detection
    const [selectedAgent, setSelectedAgent] = useState<typeof AGENT_LIST[0] | null>(null);
    const [agentStatuses, setAgentStatuses] = useState<Record<string, RemoteAgentInfo>>({});
    const [detecting, setDetecting] = useState(false);
    const [detectError, setDetectError] = useState(false);

    // Role list — loaded from CDN (same as PC scanRoles)
    const [roles, setRoles] = useState<RoleEntry[]>([]);
    const [categories, setCategories] = useState<RoleCategory[]>([]);
    const [selectedRole, setSelectedRole] = useState<RoleEntry | null>(null);
    const [setupCategory, setSetupCategory] = useState('all');
    const [rolesLoading, setRolesLoading] = useState(false);
    const [allLabel, setAllLabel] = useState('All');

    // Model — loaded from getModels (same decrypted list as PC)
    const [models, setModels] = useState<ModelConfig[]>([]);
    const [selectedModel, setSelectedModel] = useState<ModelConfig | null>(null);
    const [showModelMenu, setShowModelMenu] = useState(false);
    const [modelsLoading, setModelsLoading] = useState(false);
    const [modelWriting, setModelWriting] = useState(false);  // locks input while writing model to remote

    // Connection status — mirrors PC per-channel bridge status
    const [connectionStatus, setConnectionStatus] = useState<'standby' | 'connecting' | 'connected' | 'disconnected'>('standby');
    // Has new (unread) messages — per server, for red dot on server list
    const [hasNewMessages, setHasNewMessages] = useState<Record<string, boolean>>({});
    // Cache remote agent detection (avoid repeated SSH round-trips)
    const remoteAgentCacheRef = useRef<Record<string, any[]>>({});
    // Track last applied role per server to avoid redundant set_role calls
    const lastAppliedRoleRef = useRef<Record<string, string>>({});
    // Ref for model selector — outside click to close dropdown
    const modelSelectorRef = useRef<HTMLDivElement>(null);

    // Per-server state cache — preserves chat, agent, role, model when switching servers
    interface ServerState {
        chatMessages: ChatMsg[];
        selectedAgent: typeof AGENT_LIST[0] | null;
        selectedRole: RoleEntry | null;
        selectedModel: ModelConfig | null;
        sessionId: string | undefined;
        connectionStatus: 'standby' | 'connecting' | 'connected' | 'disconnected';
    }
    const serverStateCache = useRef<Record<string, ServerState>>({});

    // Chat
    const [message, setMessage] = useState('');
    const [chatMessages, setChatMessages] = useState<ChatMsg[]>([]);
    const [loading, setLoading] = useState(false);
    const [sessionId, setSessionId] = useState<string | undefined>(undefined);
    const chatBottomRef = useRef<HTMLDivElement>(null);

    // ── Chat persistence (same hook as PC) ──
    const diskKey = activeServer ? `mobile_${activeServer.id}` : null;
    const chToDisk = useCallback((m: ChatMsg): DiskMsg | null => {
        if (m.role === 'user' || m.role === 'ai') return { role: m.role, content: m.content };
        if (m.role === 'system') return { role: 'system', content: m.i18nKey || m.content };
        return null;
    }, []);
    const chFromDisk = useCallback((m: DiskMsg): ChatMsg => {
        if (m.role === 'system' && m.content.startsWith('error.')) {
            return { role: 'system', content: '', i18nKey: m.content };
        }
        const role = m.role === 'ai' ? 'ai' : m.role === 'user' ? 'user' : 'system';
        return { role, content: m.content };
    }, []);
    const chPrepend = useCallback((older: ChatMsg[]) => {
        setChatMessages(prev => [...older, ...prev]);
    }, []);
    const chSetMessages = useCallback((msgs: ChatMsg[]) => {
        setChatMessages(msgs);
    }, []);
    const chatPersistence = useChatPersistence<ChatMsg>({
        diskKey,
        messages: chatMessages,
        prependMessages: chPrepend,
        setMessages: chSetMessages,
        toDisk: chToDisk,
        fromDisk: chFromDisk,
    });

    // ── Load SSH servers on mount (same config as PC) ──
    useEffect(() => {
        (async () => {
            setServersLoading(true);
            try {
                const list = hasTauri() ? await api.loadSSHServers() : MOCK_SERVERS;
                setServers(list);
            } catch (err) {
                console.error('Failed to load SSH servers:', err);
                setServers([]);
            }
            setServersLoading(false);
        })();
    }, []);

    // ── Paste import: show input modal, user pastes "eb:..." code ──
    const [showPasteModal, setShowPasteModal] = useState(false);
    const [pasteInput, setPasteInput] = useState('');
    const [pasteStatus, setPasteStatus] = useState<'idle' | 'importing' | 'success' | 'error'>('idle');

    const handlePasteImport = useCallback(() => {
        setPasteInput('');
        setPasteStatus('idle');
        setShowPasteModal(true);
    }, []);

    const doPasteImport = useCallback(async (text: string) => {
        if (!text.trim().startsWith('eb:')) return;
        setPasteStatus('importing');
        try {
            const b64 = text.trim().slice(3);
            const json = decodeURIComponent(escape(atob(b64)));
            const data = JSON.parse(json);
            if (data.a !== 'echobird' || data.v < 2) { setPasteStatus('error'); return; }

            // Import SSH servers
            if (data.s?.length) {
                const current = await api.loadSSHServers();
                for (const old of current) {
                    await api.removeSSHServerFromDisk(old.id);
                }
                for (const srv of data.s) {
                    const id = `sync-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
                    await api.saveSSHServer(id, srv.h, srv.o, srv.u, srv.p, srv.n);
                }
            }
            // Import models
            if (data.m?.length) {
                const existing = await api.getModels();
                for (const em of existing) {
                    if (em.internalId !== 'local-server') {
                        await api.deleteModel(em.internalId);
                    }
                }
                for (const mod of data.m) {
                    await api.addModel({
                        name: mod.n,
                        modelId: mod.i,
                        baseUrl: mod.b,
                        apiKey: mod.k,
                        anthropicUrl: mod.x || undefined,
                    });
                }
            }
            const updated = await api.loadSSHServers();
            setServers(updated);
            setPasteStatus('success');
            setTimeout(() => setShowPasteModal(false), 800);
        } catch (e) {
            console.error('[Sync] Paste import failed:', e);
            setPasteStatus('error');
            setTimeout(() => setPasteStatus('idle'), 1500);
        }
    }, []);

    // Auto-detect: trigger import when paste input starts with 'eb:'
    const handlePasteInputChange = useCallback((val: string) => {
        setPasteInput(val);
        if (val.trim().startsWith('eb:') && pasteStatus === 'idle') {
            doPasteImport(val);
        }
    }, [pasteStatus, doPasteImport]);

    // ── Auto-scroll chat ──
    useEffect(() => {
        chatBottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [chatMessages, loading]);

    // ── Close model dropdown on tap outside ──
    useEffect(() => {
        if (!showModelMenu) return;
        const handler = (e: MouseEvent) => {
            if (modelSelectorRef.current && !modelSelectorRef.current.contains(e.target as Node)) {
                setShowModelMenu(false);
            }
        };
        document.addEventListener('mousedown', handler);
        return () => document.removeEventListener('mousedown', handler);
    }, [showModelMenu]);

    // ── Open server → go to chat ──
    const openServer = (server: SSHServer) => {
        // Save current server's state before switching
        if (activeServer) {
            serverStateCache.current[activeServer.id] = {
                chatMessages,
                selectedAgent,
                selectedRole,
                selectedModel,
                sessionId,
                connectionStatus,
            };
        }

        // Restore cached state for target server (or reset if first visit)
        const cached = serverStateCache.current[server.id];
        setActiveServer(server);
        if (cached) {
            setChatMessages(cached.chatMessages);
            setSelectedAgent(cached.selectedAgent);
            setSelectedRole(cached.selectedRole);
            setSelectedModel(cached.selectedModel);
            setSessionId(cached.sessionId);
            setConnectionStatus(cached.connectionStatus);
        } else {
            setChatMessages([]);
            setSessionId(undefined);
            setConnectionStatus('standby');
            // Restore agent/role/model from localStorage (survives app restart)
            const savedAgentId = localStorage.getItem(`mb_agent_${server.id}`);
            const savedAgent = savedAgentId ? AGENT_LIST.find(a => a.id === savedAgentId) || null : null;
            setSelectedAgent(savedAgent);
            // Restore role from localStorage (full object JSON)
            try {
                const savedRoleJson = localStorage.getItem(`mb_role_${server.id}`);
                setSelectedRole(savedRoleJson ? JSON.parse(savedRoleJson) : null);
            } catch { setSelectedRole(null); }
            setSelectedModel(null); // model will be restored after models load
            // Trigger model load for restored agent
            if (savedAgent) doLoadModels(savedAgent.id);
        }
        setShowModelMenu(false);
        // Clear hasNew for this server
        setHasNewMessages(prev => ({ ...prev, [server.id]: false }));
        navigateTo('chat');
    };

    // ── Load chat history from disk when entering server with no cached messages ──
    useEffect(() => {
        if (activeServer && chatMessages.length === 0) {
            chatPersistence.loadInitial();
        }
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [activeServer?.id]);

    // ── Restore model from localStorage after models finish loading ──
    useEffect(() => {
        if (!activeServer || selectedModel) return;
        const savedModelId = localStorage.getItem(`mb_model_${activeServer.id}`);
        if (savedModelId && models.length > 0) {
            const match = models.find(m => (m.modelId || m.name) === savedModelId);
            if (match) setSelectedModel(match);
        }
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [models.length, activeServer?.id]);

    // ── Setup: detect agents on remote server ──
    // ensure_remote_bridge is called internally by the Rust backend (bridge_detect_agents_remote)
    const doDetectAgents = useCallback(async (serverId: string) => {
        setDetecting(true);
        setDetectError(false);
        if (!hasTauri()) {
            await new Promise(r => setTimeout(r, 800));
            setAgentStatuses(MOCK_AGENTS);
            setDetecting(false);
            return;
        }
        try {
            const agents = await api.bridgeDetectAgentsRemote(serverId);
            const map: Record<string, RemoteAgentInfo> = {};
            agents.forEach(a => { map[a.id] = a; });
            setAgentStatuses(map);
        } catch (err: any) {
            console.error('Agent detection failed:', err);
            setAgentStatuses({});
            setDetectError(true);
            setSelectedAgent(null);
            setSelectedRole(null);
        }
        setDetecting(false);
    }, [servers]);

    const rolesLocaleRef = useRef('');
    const doLoadRoles = useCallback(async () => {
        if (roles.length > 0 && rolesLocaleRef.current === locale) return; // already loaded for this locale
        setRolesLoading(true);
        try {
            const result = await api.scanRoles(locale);
            setCategories(result.categories);
            setRoles(result.roles);
            setAllLabel(result.allLabel || 'All');
            rolesLocaleRef.current = locale;
        } catch (err) {
            console.error('Failed to load roles:', err);
        }
        setRolesLoading(false);
    }, [roles.length, locale]);

    // ── Load models (same decrypted list as PC, filtered by agent protocol) ──
    const doLoadModels = useCallback(async (agentId?: string) => {
        setModelsLoading(true);
        try {
            const list = hasTauri() ? await api.getModels() : MOCK_MODELS;
            // Claude Code only supports Anthropic protocol
            const anthropicOnlyAgents = ['claudecode'];
            const isAnthropicOnly = agentId && anthropicOnlyAgents.includes(agentId);
            const filtered = isAnthropicOnly ? list.filter(m => !!(m as any).anthropicUrl) : list;
            setModels(filtered);
        } catch (err) {
            console.error('Failed to load models:', err);
            setModels([]);
        }
        setModelsLoading(false);
    }, []);

    // ── Open setup screen ──
    const openSetup = () => {
        navigateTo('setup');
        setSetupCategory('all');
        doLoadRoles();
        if (activeServer) {
            doDetectAgents(activeServer.id);
        }
    };

    // ── Select agent (mirrors PC: clear model/session/cache, stop old bridge, auto-load models) ──
    const handleSelectAgent = (agentDef: typeof AGENT_LIST[0]) => {
        const info = agentStatuses[agentDef.id];
        if (!info?.installed) return;
        const previousAgent = selectedAgent;
        setSelectedAgent(agentDef);
        setSelectedRole(null);
        setSelectedModel(null);
        setModels([]);
        // Persist agent selection to localStorage
        if (activeServer) localStorage.setItem(`mb_agent_${activeServer.id}`, agentDef.id);
        // Reset bridge state on agent change
        if (previousAgent && previousAgent.id !== agentDef.id) {
            setConnectionStatus('standby');
            setSessionId(undefined);
            if (activeServer) {
                delete remoteAgentCacheRef.current[activeServer.id];
                lastAppliedRoleRef.current[activeServer.id] = '';
            }
            // Stop old bridge process
            if (hasTauri()) api.bridgeStop().catch(() => {});
        }
        // Auto-load models (filtered by agent protocol)
        doLoadModels(agentDef.id);
        // Auto-detect current model on remote (non-blocking — runs in background)
        // IMPORTANT: After detecting, immediately write model config back to remote
        // to ensure ZeroClaw's config.toml is synced with the user's Model Nexus settings.
        if (activeServer && hasTauri()) {
            setModelWriting(true);
            Promise.all([
                api.bridgeGetRemoteModel(activeServer.id, agentDef.id),
                api.getModels(),
            ]).then(async ([m, allModels]) => {
                if (m?.modelId) {
                    const match = allModels.find(mod => mod.modelId === m.modelId || mod.name === m.modelId);
                    if (match) {
                        setSelectedModel(match);
                        if (activeServer) localStorage.setItem(`mb_model_${activeServer.id}`, match.modelId || match.name);
                        // Write model to remote (same as manual model select in dropdown)
                        const anthropicOnlyAgents = ['claudecode'];
                        const isAnthropicAgent = anthropicOnlyAgents.includes(agentDef.id);
                        const effectiveBaseUrl = isAnthropicAgent
                            ? (match.anthropicUrl || match.baseUrl || '')
                            : (match.baseUrl || '');
                        const effectiveProtocol = isAnthropicAgent ? 'anthropic' : 'openai';
                        try {
                            await api.bridgeSetRemoteModel(
                                activeServer.id,
                                agentDef.id,
                                match.modelId || match.name,
                                match.name,
                                match.apiKey || '',
                                effectiveBaseUrl,
                                effectiveProtocol,
                            );
                        } catch { /* non-fatal: model was already set by PC */ }
                    }
                }
            }).catch(() => { /* model detection is optional */ })
              .finally(() => setModelWriting(false));
        }
    };

    // ── Select role ──
    const handleSelectRole = (role: RoleEntry) => {
        setSelectedRole(role);
        // Persist full role object to localStorage
        if (activeServer) localStorage.setItem(`mb_role_${activeServer.id}`, JSON.stringify(role));
        if (models.length === 0 && selectedAgent) doLoadModels(selectedAgent.id);
    };

    const handleNoRole = () => {
        setSelectedRole(null);
        if (activeServer) localStorage.removeItem(`mb_role_${activeServer.id}`);
        if (models.length === 0 && selectedAgent) doLoadModels(selectedAgent.id);
    };

    // ── Send message (mirrors PC Channels 4-step remote flow) ──
    const sendMessage = useCallback(async () => {
        if (!message.trim() || loading || modelWriting) return;

        // Guard: no agent
        if (!selectedAgent) {
            setChatMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.agentFailed' }]);
            return;
        }
        // Guard: no model
        if (!selectedModel) {
            setChatMessages(prev => [...prev, { role: 'system', content: '', i18nKey: 'error.noModelSelected' }]);
            return;
        }
        if (!activeServer) return;

        const userMsg = message.trim();
        setMessage('');
        setChatMessages(prev => [...prev, { role: 'user', content: userMsg }]);
        setLoading(true);

        try {

            const serverId = activeServer.id;
            setConnectionStatus('connecting');

            // 30s working hint — so user knows agent is still running
            const WORKING_MARKER = '__agent_working__';
            const workingTimer = setTimeout(() => {
                setChatMessages(prev => {
                    if (prev.some(m => m.content === WORKING_MARKER)) return prev;
                    return [...prev, { role: 'system', content: WORKING_MARKER }];
                });
            }, 30_000);

            try {
                // Step 1: Detect agent on remote (cached)
                // ensure_remote_bridge runs internally in Rust (bridge_detect_agents_remote)
                if (!remoteAgentCacheRef.current[serverId]) {
                    const agents = await api.bridgeDetectAgentsRemote(serverId);
                    remoteAgentCacheRef.current[serverId] = agents;
                }
                const cachedAgents = remoteAgentCacheRef.current[serverId];
                const agentInfo = cachedAgents?.find((a: any) => a.id === selectedAgent.id);
                if (!agentInfo?.installed) {
                    setConnectionStatus('standby');
                    setChatMessages(prev => [...prev, {
                        role: 'system',
                        content: '', i18nKey: 'error.agentFailed',
                    }]);
                    clearTimeout(workingTimer);
                    setLoading(false);
                    return;
                }

                // Step 2: Start agent if not running
                if (!agentInfo.running) {
                    try {
                        await api.bridgeStartAgentRemote(serverId, selectedAgent.id);
                        agentInfo.running = true;
                    } catch (e) {
                        console.warn('[Bridge] start_agent failed (non-fatal):', e);
                    }
                }

                // Step 3: Set role if selected and changed
                const roleKey = `${selectedAgent.id}:${selectedRole?.id || ''}`;
                const lastApplied = lastAppliedRoleRef.current[serverId];
                if (selectedRole?.filePath) {
                    if (roleKey !== lastApplied) {
                        try {
                            await api.bridgeSetRoleRemote(serverId, selectedAgent.id, selectedRole.id, selectedRole.filePath);
                            lastAppliedRoleRef.current[serverId] = roleKey;
                            // Force new session so agent reads updated role file
                            // (OpenClaw only reads SOUL.md at session start)
                            setSessionId(undefined);
                        } catch (e) {
                            console.warn('[Bridge] set_role failed (non-fatal):', e);
                        }
                    }
                } else if (lastApplied) {
                    // Clear role: pass actual previous role_id (not empty string)
                    const lastRoleId = lastApplied.split(':')[1] || '';
                    try {
                        await api.bridgeClearRoleRemote(serverId, selectedAgent.id, lastRoleId);
                        setSessionId(undefined);
                        lastAppliedRoleRef.current[serverId] = '';
                    } catch (e) {
                        console.warn('[Bridge] clear_role failed (non-fatal):', e);
                    }
                }

                // Step 4: Send message
                const result = await api.bridgeChatRemote(
                    serverId, userMsg, sessionId, selectedAgent.id, selectedRole?.name,
                );
                clearTimeout(workingTimer);
                setConnectionStatus('connected');
                // Remove working hint, add real reply
                setChatMessages(prev => {
                    const cleaned = prev.filter(m => m.content !== WORKING_MARKER);
                    return [...cleaned, {
                        role: 'ai',
                        content: result.text,
                        model: result.model,
                        tokens: result.tokens,
                        duration_ms: result.duration_ms,
                    }];
                });
                if (result.session_id) setSessionId(result.session_id);
                // Mark hasNew only when user is NOT actively viewing this chat
                // (red dot should only appear on server list for background conversations)
                if (screen !== 'chat') {
                    setHasNewMessages(prev => ({ ...prev, [serverId]: true }));
                }
            } catch (remoteErr: any) {
                clearTimeout(workingTimer);
                setConnectionStatus('standby');
                window.dispatchEvent(new CustomEvent('chat-error'));
                setChatMessages(prev => {
                    const cleaned = prev.filter(m => m.content !== WORKING_MARKER);
                    return [...cleaned, { role: 'system', content: '', i18nKey: errorToKey(remoteErr?.message || String(remoteErr)) }];
                });
            }
        } catch (e: any) {
            window.dispatchEvent(new CustomEvent('chat-error'));
            setChatMessages(prev => [...prev, { role: 'system', content: '', i18nKey: errorToKey(e?.message || String(e)) }]);
        } finally {
            setLoading(false);
        }
    }, [message, activeServer, loading, modelWriting, sessionId, selectedAgent, selectedRole, selectedModel]);

    // ── Filtered roles by category ──
    const filteredRoles = setupCategory === 'all'
        ? roles
        : roles.filter(r => r.category === setupCategory);

    // ── Header display ──
    const headerText = selectedRole
        ? selectedRole.name
        : selectedAgent
            ? selectedAgent.name
            : t('channel.selectRoleAgent');

    // ── Server display name ──
    const serverDisplayName = (s: SSHServer) => s.alias || `${s.username}@${s.host}`;

    return (
        <div className="mobile-app">
            {/* ===== Server List ===== */}
            {screen === 'servers' && (
                <div className={`mobile-screen ${slideDirection}`}>
                    <div className="mobile-header">
                        <button className="mobile-icon-btn" onClick={() => navigateTo('settings')}>
                            <Settings size={20} />
                        </button>
                        <div className="mobile-header-spacer" />
                        <button className="mobile-icon-btn" onClick={handlePasteImport}>
                            <ClipboardPaste size={20} />
                        </button>
                    </div>

                    {/* Paste import bar */}
                    {showPasteModal && (
                        <div style={{
                            display: 'flex', gap: 8, padding: '8px 12px',
                            borderBottom: '1px solid rgba(0,255,157,0.15)',
                            background: 'rgba(0,0,0,0.3)',
                            alignItems: 'center',
                        }}>
                            <input
                                type="text"
                                value={pasteInput}
                                onChange={e => handlePasteInputChange(e.target.value)}
                                autoFocus
                                style={{
                                    flex: 1, background: 'rgba(255,255,255,0.06)',
                                    border: `1px solid ${pasteStatus === 'success' ? 'rgba(0,255,157,0.5)' : pasteStatus === 'error' ? 'rgba(255,80,80,0.5)' : 'rgba(0,255,157,0.2)'}`,
                                    borderRadius: 8, padding: '8px 12px',
                                    color: '#e0e0e0', fontSize: 13,
                                    fontFamily: 'monospace', outline: 'none',
                                }}
                            />
                            {pasteStatus === 'importing' && <Loader2 size={18} className="spin" style={{ color: '#00ff9d', flexShrink: 0 }} />}
                            {pasteStatus === 'success' && <Check size={18} style={{ color: '#00ff9d', flexShrink: 0 }} />}
                            {pasteStatus === 'error' && <X size={18} style={{ color: '#ff5050', flexShrink: 0 }} />}
                            {pasteStatus === 'idle' && (
                                <button
                                    onClick={() => { setShowPasteModal(false); setPasteInput(''); }}
                                    style={{
                                        background: 'none', border: 'none',
                                        color: 'rgba(255,255,255,0.3)', cursor: 'pointer',
                                        display: 'flex', alignItems: 'center',
                                        padding: 0, flexShrink: 0,
                                    }}
                                >
                                    <X size={18} />
                                </button>
                            )}
                        </div>
                    )}

                    {serversLoading ? (
                        <div className="empty-servers">
                            <Loader2 size={32} className="spin" />
                        </div>
                    ) : servers.length === 0 ? (
                        <div className="empty-servers">
                            <img src="/ico.png" alt="" className="empty-servers-watermark" />
                        </div>
                    ) : (
                        <OverscrollWrap className="server-list">
                            {servers.map(s => {
                                const isActive = activeServer?.id === s.id;
                                const isThisServerActive = isActive && screen !== 'servers';
                                const serverConnStatus = isActive ? connectionStatus : 'standby';
                                const isLinked = serverConnStatus === 'connected';
                                const isConnecting = serverConnStatus === 'connecting';
                                const isError = serverConnStatus === 'disconnected';
                                const isTyping = isActive && loading;
                                const hasNew = hasNewMessages[s.id] && !isActive;
                                // Show selected agent icon or "?" fallback
                                const serverAgent = isActive && selectedAgent ? selectedAgent : null;
                                return (
                                    <div
                                        key={s.id}
                                        className={`server-card ${isActive ? 'active' : ''}`}
                                        onClick={() => openServer(s)}
                                    >
                                        <div className="server-card-row">
                                            <div className={`server-card-icon${serverAgent ? '' : ' fallback'}`}>
                                                {serverAgent ? (
                                                    <img src={serverAgent.icon} alt="" className="server-agent-img" />
                                                ) : (
                                                    <span className="server-card-icon-fallback">?</span>
                                                )}
                                            </div>
                                            <div className="server-card-info">
                                                <div className="server-card-name-row">
                                                    <span className={`server-card-name ${isActive ? 'active' : ''}`}>
                                                        {serverDisplayName(s)}
                                                    </span>
                                                    <div className={`server-status-dot ${hasNew ? 'has-new' : isLinked ? 'connected' : isConnecting ? 'connecting' : isError ? 'error' : 'standby'}`} />
                                                </div>
                                                <div className="server-card-sub-row">
                                                    <span className={`server-card-status ${isTyping ? 'typing' : isLinked ? 'connected' : isConnecting ? 'connecting' : isError ? 'error' : ''}`}>
                                                        [{isTyping ? t('common.inputting') : isLinked ? t('channel.linked') : isConnecting ? t('channel.connecting') : isError ? t('channel.failed') : t('channel.standby')}]
                                                    </span>
                                                    {(isActive && selectedRole) && (
                                                        <span className="server-card-sub">{selectedRole.name}</span>
                                                    )}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                );
                            })}
                        </OverscrollWrap>
                    )}
                </div>
            )}

            {/* ===== Chat (Telegram Layout) ===== */}
            {screen === 'chat' && activeServer && (
                <div className={`mobile-screen ${slideDirection}`}>
                    {/* Header: [←] [Role Name ▾] [Agent Icon] */}
                    <div className="chat-header">
                        <button className="mobile-icon-btn" onClick={() => navigateTo('servers')}>
                            <ChevronLeft size={22} />
                        </button>
                        <div className="chat-header-center" onClick={openSetup}>
                            <div className="chat-header-title">
                                {headerText}
                                <span className="chat-header-arrow">▾</span>
                            </div>
                            {selectedAgent && selectedRole && (
                                <div className="chat-header-subtitle">
                                    {selectedAgent.name}
                                </div>
                            )}
                        </div>
                        <div className="chat-header-avatar" onClick={openSetup}>
                            {selectedAgent ? (
                                <img
                                    src={selectedAgent.icon}
                                    alt=""
                                    className="chat-avatar-img"
                                    onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }}
                                />
                            ) : (
                                <span className="chat-avatar-fallback">?</span>
                            )}
                        </div>
                    </div>

                    {/* Messages */}
                    <OverscrollWrap className="chat-messages">

                        {chatMessages.map((msg, i) => {
                            // Hide __agent_working__ system marker (same as PC)
                            if (msg.role === 'system' && msg.content === '__agent_working__') return null;
                            return (
                            <div key={i} className={`chat-bubble ${msg.role}`}>
                                {msg.role === 'user' && (
                                    <svg className="bubble-tail-right" width="8" height="14" viewBox="0 0 8 14">
                                        <path d="M0,2 C0,1 0.8,0.4 1.5,1 L6.5,6 C7.2,6.6 7.2,7.4 6.5,8 L1.5,13 C0.8,13.6 0,13 0,12 Z" fill="#00FF9D"/>
                                    </svg>
                                )}
                                {msg.role === 'ai' && (
                                    <svg className="bubble-tail-left" width="8" height="14" viewBox="0 0 8 14">
                                        <path d="M8,2 C8,1 7.2,0.4 6.5,1 L1.5,6 C0.8,6.6 0.8,7.4 1.5,8 L6.5,13 C7.2,13.6 8,13 8,12 Z" fill="#2A2A2A"/>
                                    </svg>
                                )}
                                {msg.i18nKey ? t(msg.i18nKey as any) : msg.content}
                            </div>
                            );
                        })}
                        {loading && (
                            <div className="chat-bubble ai">
                                <svg className="bubble-tail-left" width="8" height="14" viewBox="0 0 8 14">
                                    <path d="M8,2 C8,1 7.2,0.4 6.5,1 L1.5,6 C0.8,6.6 0.8,7.4 1.5,8 L6.5,13 C7.2,13.6 8,13 8,12 Z" fill="#2A2A2A"/>
                                </svg>
                                <span className="typing-indicator">
                                    <span className="typing-indicator-text">{t('common.inputting')}</span>
                                    <span className="typing-dots">
                                        <span className="typing-dot" />
                                        <span className="typing-dot" />
                                        <span className="typing-dot" />
                                    </span>
                                </span>
                            </div>
                        )}
                        <div ref={chatBottomRef} />
                    </OverscrollWrap>

                    {/* Input area — [model-icon] [📎 input ▶] */}
                    <div className="chat-input-wrap">
                        {/* Model icon — independent, left of input box */}
                        {selectedAgent && (
                            <div className="model-selector-wrapper" ref={modelSelectorRef}>
                                <button
                                    className="chat-model-btn"
                                    onClick={() => {
                                        if (modelWriting) return;
                                        if (models.length === 0 && selectedAgent) doLoadModels(selectedAgent.id);
                                        setShowModelMenu(!showModelMenu);
                                    }}
                                    disabled={modelWriting}
                                    title={selectedModel?.name || t('mother.selectModel')}
                                >
                                    {modelWriting ? (
                                        <Loader2 size={20} className="spin" />
                                    ) : selectedModel ? (
                                        (() => {
                                            const iconPath = getModelIcon(selectedModel.name, selectedModel.modelId);
                                            return iconPath ? (
                                                <img
                                                    src={iconPath}
                                                    alt=""
                                                    className="model-icon-img"
                                                    onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }}
                                                />
                                            ) : (
                                                <Box size={20} />
                                            );
                                        })()
                                    ) : (
                                        <Box size={20} />
                                    )}
                                </button>
                                {showModelMenu && (
                                    <div className="model-dropdown">
                                        {modelsLoading ? (
                                            <div className="model-dropdown-item">
                                                <Loader2 size={14} className="spin" />
                                            </div>
                                        ) : models.length === 0 ? (
                                            <div className="model-dropdown-item">
                                                No models configured
                                            </div>
                                        ) : (
                                            models.map(m => (
                                                <div
                                                    key={m.internalId}
                                                    className={`model-dropdown-item ${selectedModel?.internalId === m.internalId ? 'active' : ''}`}
                                                    onClick={async () => {
                                                        const prevModel = selectedModel;
                                                        setSelectedModel(m);
                                                        setShowModelMenu(false);
                                                        if (activeServer) localStorage.setItem(`mb_model_${activeServer.id}`, m.modelId || m.name);
                                                        // Write model to remote — lock input during write
                                                        setModelWriting(true);
                                                        try {
                                                            if (hasTauri() && activeServer && selectedAgent) {
                                                                const anthropicOnlyAgents = ['claudecode'];
                                                                const isAnthropicAgent = anthropicOnlyAgents.includes(selectedAgent.id);
                                                                const effectiveBaseUrl = isAnthropicAgent
                                                                    ? (m.anthropicUrl || m.baseUrl || '')
                                                                    : (m.baseUrl || '');
                                                                const effectiveProtocol = isAnthropicAgent ? 'anthropic' : 'openai';
                                                                await api.bridgeSetRemoteModel(
                                                                    activeServer.id,
                                                                    selectedAgent.id,
                                                                    m.modelId || m.name,
                                                                    m.name,
                                                                    m.apiKey || '',
                                                                    effectiveBaseUrl,
                                                                    effectiveProtocol,
                                                                );
                                                            }
                                                        } catch {
                                                            // Rollback on failure
                                                            setSelectedModel(prevModel);
                                                        } finally {
                                                            setModelWriting(false);
                                                        }
                                                    }}
                                                >
                                                    {(() => {
                                                        const iconPath = getModelIcon(m.name, m.modelId);
                                                        return iconPath ? (
                                                            <img src={iconPath} alt="" className="model-dropdown-icon" onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }} />
                                                        ) : null;
                                                    })()}
                                                    <span className="model-dropdown-name">{m.name}</span>
                                                    {selectedModel?.internalId === m.internalId && <span className="model-check">✓</span>}
                                                </div>
                                            ))
                                        )}
                                    </div>
                                )}
                            </div>
                        )}
                        {/* Input box: [input text only] */}
                        <div className="chat-input-box">
                            <textarea
                                className="chat-textarea"
                                placeholder={modelWriting ? '...' : loading ? t('channel.awaitingResponse') : t('channel.enterMessage')}
                                value={message}
                                onChange={e => setMessage(e.target.value)}
                                onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); } }}
                                disabled={loading || modelWriting}
                                rows={1}
                            />
                        </div>
                        {loading ? (
                            <button className="chat-send-circle abort" onClick={() => setLoading(false)}>
                                <Loader2 size={20} className="spin" />
                            </button>
                        ) : (
                            <button className="chat-send-circle" onClick={sendMessage} disabled={!message.trim() || modelWriting}>
                                <Send size={20} />
                            </button>
                        )}
                    </div>
                </div>
            )}

            {/* ===== Setup Screen (= PC AgentRolePicker) ===== */}
            {screen === 'setup' && activeServer && (
                <div className={`mobile-screen ${slideDirection}`}>
                    <div className="mobile-header">
                        <button className="mobile-icon-btn" onClick={() => navigateTo('chat')}>
                            <ChevronLeft size={22} />
                        </button>
                        <h2 className="mobile-title">
                            {t('channel.selectRoleAgent')}
                            {detecting && (
                                <Loader2 size={16} className="spin" style={{ marginLeft: 8, display: 'inline-block', verticalAlign: 'middle' }} />
                            )}
                            {detectError && !detecting && (
                                <span className="detect-error-text">{t('error.serverUnreachable')}</span>
                            )}
                        </h2>
                        <div className="mobile-header-spacer" />
                    </div>

                    {/* Agent row — horizontal scroll, installed first */}
                    <div className="setup-agent-row">
                        {[...AGENT_LIST].sort((a, b) => {
                            const aInstalled = agentStatuses[a.id]?.installed ? 1 : 0;
                            const bInstalled = agentStatuses[b.id]?.installed ? 1 : 0;
                            return bInstalled - aInstalled;
                        }).map(agentDef => {
                            const info = agentStatuses[agentDef.id];
                            const installed = info?.installed ?? false;
                            const active = selectedAgent?.id === agentDef.id;
                            return (
                                <button
                                    key={agentDef.id}
                                    className={`setup-agent-chip ${active ? 'active' : ''} ${!installed && !detecting ? 'disabled' : ''} ${detecting ? 'detecting' : ''}`}
                                    onClick={() => !detecting && handleSelectAgent(agentDef)}
                                    disabled={detecting || !installed}
                                >
                                    <img src={agentDef.icon} alt="" className="setup-agent-icon" onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }} />
                                    <span>{agentDef.name}</span>
                                    {info?.running && (
                                        <svg viewBox="0 0 24 24" className="agent-running-dot" aria-hidden="true">
                                            <path d="M12 21.593c-5.63-5.539-11-10.297-11-14.402 0-3.791 3.068-5.191 5.281-5.191 1.312 0 4.151.501 5.719 4.457 1.59-3.968 4.464-4.447 5.726-4.447 2.54 0 5.274 1.621 5.274 5.181 0 4.069-5.136 8.625-11 14.402z" fill="currentColor" />
                                        </svg>
                                    )}
                                </button>
                            );
                        })}
                    </div>

                    {/* Category tabs — horizontal scroll */}
                    <div className="setup-category-row">
                        <button
                            className={`setup-category-tab ${setupCategory === 'all' ? 'active' : ''}`}
                            onClick={() => setSetupCategory('all')}
                        >
                            {allLabel}
                        </button>
                        {categories.map(cat => (
                            <button
                                key={cat.id}
                                className={`setup-category-tab ${setupCategory === cat.id ? 'active' : ''}`}
                                onClick={() => setSetupCategory(cat.id)}
                            >
                                {cat.label || cat.name}
                            </button>
                        ))}
                    </div>

                    {/* Role card grid — PC style */}
                    <OverscrollWrap className="setup-role-grid-wrap">
                        {rolesLoading ? (
                            <div className="setup-detecting">
                                <Loader2 size={24} className="spin" />
                            </div>
                        ) : (
                            <div className="setup-role-grid">
                                {/* No Role card */}
                                <div
                                    className={`role-card ${!selectedRole ? 'selected' : ''}`}
                                    onClick={handleNoRole}
                                >
                                    <div className="role-card-img">
                                        <img src="/none.png" alt="" className="role-card-image" style={{ opacity: 1 }} />
                                    </div>
                                    {!selectedRole && (
                                        <div className="role-card-check">
                                            <Check size={12} strokeWidth={3} />
                                        </div>
                                    )}
                                </div>

                                {filteredRoles.map(role => {
                                    const isSelected = selectedRole?.id === role.id;
                                    const isDisabled = !selectedAgent;
                                    return (
                                        <div
                                            key={role.id}
                                            className={`role-card ${isSelected ? 'selected' : ''} ${isDisabled ? 'disabled' : ''}`}
                                            onClick={() => !isDisabled && handleSelectRole(role)}
                                        >
                                            <div className="role-card-img">
                                                <div className="role-card-skeleton" />
                                                <img
                                                    src={role.img || role.fallbackImg}
                                                    alt={role.name}
                                                    className="role-card-image"
                                                    loading="lazy"
                                                    onLoad={e => { (e.target as HTMLImageElement).style.opacity = '1'; }}
                                                    onError={e => {
                                                        const el = e.target as HTMLImageElement;
                                                        if (role.fallbackImg && el.src !== role.fallbackImg) {
                                                            el.src = role.fallbackImg;
                                                        } else if (!el.src.endsWith('/none.png')) {
                                                            el.src = '/none.png';
                                                        }
                                                    }}
                                                />
                                            </div>
                                            {isSelected && (
                                                <div className="role-card-check">
                                                    <Check size={12} strokeWidth={3} />
                                                </div>
                                            )}
                                            <div className="role-card-overlay" />
                                            <div className="role-card-text">
                                                <div className="role-card-name">{role.name}</div>
                                                <div className="role-card-desc">{role.description}</div>
                                            </div>
                                        </div>
                                    );
                                })}
                            </div>
                        )}
                    </OverscrollWrap>
                </div>
            )}

            {/* ===== Settings ===== */}


            {/* ===== Settings ===== */}
            {screen === 'settings' && <SettingsScreen locale={locale} setLocale={setLocale} onBack={() => navigateTo('servers')} t={t} slideClass={slideDirection} />}
        </div>
    );
}

export default MobileApp;
