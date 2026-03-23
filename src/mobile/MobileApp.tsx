// MobileApp.tsx — Mobile-only application shell (vertical Channels page)
// Uses the SAME Tauri APIs and config files as PC Channels page.
// Flow: Server List (from loadSSHServers) → Chat (Telegram layout)
// Setup: tap header → Agent detection (bridgeDetectAgentsRemote) + Role list (scanRoles CDN)
// Model: bottom bar selector (bridgeGetRemoteModel / getModels)

import { useState, useEffect, useRef, useCallback } from 'react';
import { ArrowLeft, Settings, Send, Loader2, Paperclip } from 'lucide-react';
import * as api from '../api/tauri';
import type { SSHServer, RemoteAgentInfo, RoleEntry, RoleCategory } from '../api/tauri';
import type { ModelConfig } from '../api/types';
import './MobileApp.css';

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

// Mock data for browser dev testing (when Tauri backend unavailable)
const MOCK_SERVERS: SSHServer[] = [
    { id: '1774143154496', host: '192.168.10.39', port: 22, username: 'eben', password: 'enc:v1:mock', alias: '1060' },
    { id: 'mock_redmi', host: '192.168.10.50', port: 22, username: 'root', password: 'enc:v1:mock', alias: 'Redmi' },
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
    { internalId: 'mock-1', name: 'MiniMax-M2.7', baseUrl: 'https://api.minimax.chat/v1', apiKey: '' },
    { internalId: 'mock-2', name: 'GLM-5', baseUrl: 'https://open.bigmodel.cn/v1', apiKey: '' },
    { internalId: 'mock-3', name: 'Qwen3-Max', baseUrl: 'https://dashscope.aliyuncs.com/v1', apiKey: '' },
];

interface ChatMsg {
    role: 'user' | 'ai' | 'system';
    content: string;
    model?: string;
    tokens?: number;
    duration_ms?: number;
}

interface QRPayload {
    app: 'echobird';
    v: number;
    servers: { name: string; address: string; serverId?: string }[];
}

function MobileApp() {
    const [screen, setScreen] = useState<MobileScreen>('servers');

    // Server list — loaded from same config as PC (ssh_servers.json)
    const [servers, setServers] = useState<SSHServer[]>([]);
    const [serversLoading, setServersLoading] = useState(true);
    const [activeServer, setActiveServer] = useState<SSHServer | null>(null);

    // Agent detection
    const [selectedAgent, setSelectedAgent] = useState<typeof AGENT_LIST[0] | null>(null);
    const [agentStatuses, setAgentStatuses] = useState<Record<string, RemoteAgentInfo>>({});
    const [detecting, setDetecting] = useState(false);

    // Role list — loaded from CDN (same as PC scanRoles)
    const [roles, setRoles] = useState<RoleEntry[]>([]);
    const [categories, setCategories] = useState<RoleCategory[]>([]);
    const [selectedRole, setSelectedRole] = useState<RoleEntry | null>(null);
    const [setupCategory, setSetupCategory] = useState('all');
    const [rolesLoading, setRolesLoading] = useState(false);

    // Model — loaded from getModels (same decrypted list as PC)
    const [models, setModels] = useState<ModelConfig[]>([]);
    const [selectedModel, setSelectedModel] = useState<ModelConfig | null>(null);
    const [showModelMenu, setShowModelMenu] = useState(false);
    const [modelsLoading, setModelsLoading] = useState(false);

    // Chat
    const [message, setMessage] = useState('');
    const [chatMessages, setChatMessages] = useState<ChatMsg[]>([]);
    const [loading, setLoading] = useState(false);
    const [sessionId, setSessionId] = useState<string | undefined>(undefined);
    const chatBottomRef = useRef<HTMLDivElement>(null);

    // ── Load SSH servers on mount (same config as PC) ──
    useEffect(() => {
        (async () => {
            setServersLoading(true);
            try {
                const list = hasTauri() ? await api.loadSSHServers() : MOCK_SERVERS;
                setServers(list);
            } catch (err) {
                console.error('Failed to load SSH servers:', err);
                setServers(MOCK_SERVERS);
            }
            setServersLoading(false);
        })();
    }, []);

    // ── Auto-scroll chat ──
    useEffect(() => {
        chatBottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [chatMessages, loading]);

    // ── Open server → go to chat ──
    const openServer = (server: SSHServer) => {
        setActiveServer(server);
        setSelectedAgent(null);
        setSelectedRole(null);
        setSelectedModel(null);
        setShowModelMenu(false);
        setChatMessages([]);
        setSessionId(undefined);
        setScreen('chat');
    };

    // ── Setup: detect agents on remote server ──
    const doDetectAgents = useCallback(async (serverId: string) => {
        setDetecting(true);
        if (!hasTauri()) {
            // Browser dev: simulate detection delay + mock data
            await new Promise(r => setTimeout(r, 800));
            setAgentStatuses(MOCK_AGENTS);
            setDetecting(false);
            return;
        }
        try {
            await api.bridgeEnsureRemote(serverId);
            const agents = await api.bridgeDetectAgentsRemote(serverId);
            const map: Record<string, RemoteAgentInfo> = {};
            agents.forEach(a => { map[a.id] = a; });
            setAgentStatuses(map);
        } catch (err) {
            console.error('Agent detection failed:', err);
            setAgentStatuses({});
        }
        setDetecting(false);
    }, []);

    // ── Load roles from CDN (same as PC scanRoles) ──
    const doLoadRoles = useCallback(async () => {
        if (roles.length > 0) return; // already loaded
        setRolesLoading(true);
        try {
            const result = await api.scanRoles('en');
            setCategories(result.categories);
            setRoles(result.roles);
        } catch (err) {
            console.error('Failed to load roles:', err);
        }
        setRolesLoading(false);
    }, [roles.length]);

    // ── Load models (same decrypted list as PC) ──
    const doLoadModels = useCallback(async () => {
        setModelsLoading(true);
        try {
            const list = hasTauri() ? await api.getModels() : MOCK_MODELS;
            setModels(list);
        } catch (err) {
            console.error('Failed to load models:', err);
            setModels(MOCK_MODELS); // fallback
        }
        setModelsLoading(false);
    }, []);

    // ── Open setup screen ──
    const openSetup = () => {
        setScreen('setup');
        setSetupCategory('all');
        doLoadRoles();
        if (activeServer) {
            doDetectAgents(activeServer.id);
        }
    };

    // ── Select agent ──
    const handleSelectAgent = async (agentDef: typeof AGENT_LIST[0]) => {
        const info = agentStatuses[agentDef.id];
        if (!info?.installed) return;
        setSelectedAgent(agentDef);
        setSelectedRole(null);
        setSelectedModel(null);
        // Auto-detect model after selecting agent
        if (activeServer) {
            try {
                const m = await api.bridgeGetRemoteModel(activeServer.id, agentDef.id);
                if (m) {
                    // Try to find matching model in our list
                    const match = models.find(mod => mod.internalId === m.modelId || mod.name === m.modelName);
                    if (match) setSelectedModel(match);
                }
            } catch { /* model detection is optional */ }
        }
    };

    // ── Select role ──
    const handleSelectRole = (role: RoleEntry) => {
        setSelectedRole(role);
        setScreen('chat');
        // Load models when entering chat with agent+role
        if (models.length === 0) doLoadModels();
    };

    const handleNoRole = () => {
        setSelectedRole(null);
        setScreen('chat');
        if (models.length === 0) doLoadModels();
    };

    // ── Send message ──
    const sendMessage = useCallback(async () => {
        if (!message.trim() || loading) return;

        if (!selectedAgent) {
            setChatMessages(prev => [...prev, {
                role: 'system',
                content: 'Please select a CLI Agent first — tap the header above',
            }]);
            return;
        }

        if (!activeServer) return;

        const userMsg = message.trim();
        setMessage('');
        setChatMessages(prev => [...prev, { role: 'user', content: userMsg }]);
        setLoading(true);

        try {
            if (!hasTauri()) {
                // Browser dev: simulate AI response
                await new Promise(r => setTimeout(r, 1000));
                setChatMessages(prev => [...prev, {
                    role: 'ai',
                    content: `[Dev] Echo: ${userMsg}\n\nAgent: ${selectedAgent.name}\nRole: ${selectedRole?.name || 'None'}\nModel: ${selectedModel?.name || 'Not selected'}`,
                    model: selectedModel?.name || 'dev-echo',
                    tokens: Math.floor(Math.random() * 500) + 100,
                    duration_ms: Math.floor(Math.random() * 3000) + 500,
                }]);
                setLoading(false);
                return;
            }

            const result = await api.bridgeChatRemote(
                activeServer.id,
                userMsg,
                sessionId ?? undefined,
                selectedAgent.id,
                selectedRole?.id ?? undefined,
            );

            setChatMessages(prev => [...prev, {
                role: 'ai',
                content: result.text,
                model: result.model,
                tokens: result.tokens,
                duration_ms: result.duration_ms,
            }]);

            if (result.session_id) setSessionId(result.session_id);
        } catch (err: any) {
            setChatMessages(prev => [...prev, {
                role: 'system',
                content: err?.message || String(err),
            }]);
        } finally {
            setLoading(false);
        }
    }, [message, activeServer, loading, sessionId, selectedAgent, selectedRole]);

    // ── Filtered roles by category ──
    const filteredRoles = setupCategory === 'all'
        ? roles
        : roles.filter(r => r.category === setupCategory);

    // ── Header display ──
    const headerText = selectedRole
        ? selectedRole.name
        : selectedAgent
            ? selectedAgent.name
            : 'Select Role and CLI Agent';

    // ── Server display name ──
    const serverDisplayName = (s: SSHServer) => s.alias || `${s.username}@${s.host}`;

    return (
        <div className="mobile-app">
            {/* ===== Server List ===== */}
            {screen === 'servers' && (
                <div className="mobile-screen">
                    <div className="mobile-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('settings')}>
                            <Settings size={20} />
                        </button>
                        <div className="mobile-header-spacer" />
                    </div>

                    {serversLoading ? (
                        <div className="empty-servers">
                            <Loader2 size={32} className="spin" />
                            <p className="empty-servers-desc">Loading servers...</p>
                        </div>
                    ) : servers.length === 0 ? (
                        <div className="empty-servers">
                            <div className="empty-servers-icon">📱</div>
                            <p className="empty-servers-title">No servers yet</p>
                            <p className="empty-servers-desc">
                                Add a server on PC first
                            </p>
                        </div>
                    ) : (
                        <div className="server-list">
                            {servers.map(s => {
                                const isActive = activeServer?.id === s.id;
                                return (
                                    <div
                                        key={s.id}
                                        className={`server-card ${isActive ? 'active' : ''}`}
                                        onClick={() => openServer(s)}
                                    >
                                        <div className="server-card-row">
                                            {/* Agent icon — transparent bg like PC */}
                                            <div className="server-card-icon">
                                                <span className="server-card-icon-fallback">?</span>
                                            </div>
                                            <div className="server-card-info">
                                                <div className="server-card-name-row">
                                                    <span className={`server-card-name ${isActive ? 'active' : ''}`}>
                                                        {serverDisplayName(s)}
                                                    </span>
                                                    <div className="server-status-dot standby" />
                                                </div>
                                                <div className="server-card-sub-row">
                                                    <span className="server-card-status">[standby]</span>
                                                    <span className="server-card-sub">{s.host}:{s.port}</span>
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    )}
                </div>
            )}

            {/* ===== Chat (Telegram Layout) ===== */}
            {screen === 'chat' && activeServer && (
                <div className="mobile-screen">
                    {/* Header: [←] [Role Name ▾] [Agent Icon] */}
                    <div className="chat-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('servers')}>
                            <ArrowLeft size={20} />
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
                    <div className="chat-messages">

                        {chatMessages.map((msg, i) => (
                            <div key={i} className={`chat-bubble ${msg.role}`}>
                                {msg.content}
                                {msg.role === 'ai' && msg.model && (
                                    <div className="chat-meta">
                                        {msg.model}{msg.tokens ? ` · ${msg.tokens} tokens` : ''}
                                        {msg.duration_ms ? ` · ${(msg.duration_ms / 1000).toFixed(1)}s` : ''}
                                    </div>
                                )}
                            </div>
                        ))}
                        {loading && (
                            <div className="chat-bubble ai">
                                <Loader2 size={16} className="spin" />
                            </div>
                        )}
                        <div ref={chatBottomRef} />
                    </div>

                    {/* Input area — single line: [📎] [input] [model] [▶] */}
                    <div className="chat-input-wrap">
                        <div className="chat-input-box">
                            <button className="chat-tool-btn"><Paperclip size={15} /></button>
                            <textarea
                                className="chat-textarea"
                                placeholder={loading ? 'Awaiting response…' : 'Enter message…'}
                                value={message}
                                onChange={e => setMessage(e.target.value)}
                                onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); } }}
                                disabled={loading}
                                rows={1}
                            />
                            {selectedAgent && (
                                <div className="model-selector-wrapper">
                                    <button
                                        className="chat-tool-btn model-icon-btn"
                                        onClick={() => {
                                            if (models.length === 0) doLoadModels();
                                            setShowModelMenu(!showModelMenu);
                                        }}
                                        title={selectedModel?.name || 'Select model'}
                                    >
                                        {selectedModel ? (
                                            <img
                                                src={`/icons/models/${selectedModel.name?.toLowerCase().split(/[\s-]/)[0] || 'default'}.svg`}
                                                alt=""
                                                className="model-icon-img"
                                                onError={e => { (e.target as HTMLImageElement).style.display = 'none'; (e.target as HTMLImageElement).nextElementSibling && ((e.target as HTMLImageElement).parentElement!.innerHTML = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/></svg>'); }}
                                            />
                                        ) : (
                                            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/></svg>
                                        )}
                                    </button>
                                    {showModelMenu && (
                                        <div className="model-dropdown">
                                            {modelsLoading ? (
                                                <div className="model-dropdown-item">
                                                    <Loader2 size={14} className="spin" /> Loading...
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
                                                            setSelectedModel(m);
                                                            setShowModelMenu(false);
                                                            if (hasTauri() && activeServer && selectedAgent) {
                                                                try {
                                                                    await api.bridgeSetRemoteModel(
                                                                        activeServer.id,
                                                                        selectedAgent.id,
                                                                        m.internalId,
                                                                        m.name,
                                                                        m.apiKey || '',
                                                                        m.baseUrl || '',
                                                                        m.anthropicUrl ? 'anthropic' : 'openai',
                                                                    );
                                                                } catch { /* model write error */ }
                                                            }
                                                        }}
                                                    >
                                                        {m.name}
                                                        {selectedModel?.internalId === m.internalId && <span className="model-check">✓</span>}
                                                    </div>
                                                ))
                                            )}
                                        </div>
                                    )}
                                </div>
                            )}
                            {loading ? (
                                <button className="chat-send-btn abort" onClick={() => setLoading(false)}>
                                    <Loader2 size={15} className="spin" />
                                </button>
                            ) : (
                                <button className="chat-send-btn" onClick={sendMessage} disabled={!message.trim()}>
                                    <Send size={15} />
                                </button>
                            )}
                        </div>
                    </div>
                </div>
            )}

            {/* ===== Setup Screen (= PC AgentRolePicker) ===== */}
            {screen === 'setup' && activeServer && (
                <div className="mobile-screen">
                    <div className="mobile-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('chat')}>
                            <ArrowLeft size={20} />
                        </button>
                        <h2 className="mobile-title">Select Role and CLI Agent</h2>
                        <button className="mobile-icon-btn" onClick={() => setScreen('chat')}>
                            ✕
                        </button>
                    </div>

                    {/* Agent row — horizontal scroll */}
                    <div className="setup-agent-row">
                        {AGENT_LIST.map(agentDef => {
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
                                    {info?.running && <span className="agent-running-dot" />}
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
                            All
                        </button>
                        {categories.map(cat => (
                            <button
                                key={cat.id}
                                className={`setup-category-tab ${setupCategory === cat.id ? 'active' : ''}`}
                                onClick={() => setSetupCategory(cat.id)}
                            >
                                {cat.name}
                            </button>
                        ))}
                    </div>

                    {/* Role list */}
                    <div className="setup-role-list">
                        {/* No Role option */}
                        <div
                            className={`setup-role-card ${!selectedRole ? 'active' : ''}`}
                            onClick={handleNoRole}
                        >
                            <div className="setup-role-name">No Role</div>
                            <div className="setup-role-desc">Chat without a preset role</div>
                        </div>

                        {rolesLoading || detecting ? (
                            <div className="setup-detecting">
                                <Loader2 size={24} className="spin" />
                                <span>{detecting ? 'Detecting agents...' : 'Loading roles...'}</span>
                            </div>
                        ) : (
                            filteredRoles.map(role => (
                                <div
                                    key={role.id}
                                    className={`setup-role-card ${selectedRole?.id === role.id ? 'active' : ''} ${!selectedAgent ? 'disabled' : ''}`}
                                    onClick={() => selectedAgent && handleSelectRole(role)}
                                >
                                    <div className="setup-role-name">{role.name}</div>
                                    <div className="setup-role-desc">{role.category}</div>
                                </div>
                            ))
                        )}
                    </div>
                </div>
            )}

            {/* ===== Settings ===== */}
            {screen === 'settings' && (
                <div className="mobile-screen">
                    <div className="mobile-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('servers')}>
                            <ArrowLeft size={20} />
                        </button>
                        <h2 className="mobile-title">Settings</h2>
                        <div className="mobile-header-spacer" />
                    </div>
                    <div className="settings-list">
                        <div className="settings-item">
                            <span>Version</span>
                            <span className="settings-value">3.2.7</span>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}

export default MobileApp;
