// MobileApp.tsx — Mobile-only application shell
// Flow: Server List → Setup (Agent+Role) → Chat
// Matches PC Channels page logic

import { useState, useEffect, useRef, useCallback } from 'react';
import { ArrowLeft, Settings, Plus, Send, Loader2 } from 'lucide-react';
import { Html5Qrcode } from 'html5-qrcode';
import { invoke } from '@tauri-apps/api/core';
import './MobileApp.css';

type MobileScreen = 'servers' | 'qr' | 'setup' | 'chat' | 'settings';

// Agent list — same as PC client (hardcoded)
const AGENT_LIST = [
    { id: 'openclaw', name: 'OpenClaw', icon: '/icons/tools/openclaw.svg' },
    { id: 'claudecode', name: 'Claude Code', icon: '/icons/tools/claudecode.svg' },
    { id: 'zeroclaw', name: 'ZeroClaw', icon: '/icons/tools/zeroclaw.png' },
    { id: 'nanobot', name: 'NanoBot', icon: '/icons/tools/nanobot.png' },
    { id: 'picoclaw', name: 'PicoClaw', icon: '/icons/tools/picoclaw.png' },
    { id: 'hermes', name: 'Hermes Agent', icon: '/icons/tools/hermes.png' },
];

// Role list — hardcoded subset (same as PC)
const ROLE_LIST = [
    { id: 'none', name: 'No Role' },
    { id: 'developer', name: 'Developer' },
    { id: 'writer', name: 'Writer' },
    { id: 'translator', name: 'Translator' },
    { id: 'analyst', name: 'Analyst' },
];

interface ServerEntry {
    id: string;
    name: string;
    address: string;
    serverId?: string;
    lastMessage: string;
    lastTime: string;
}

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

function qrToServers(data: QRPayload): ServerEntry[] {
    return data.servers.map((srv, i) => ({
        id: `qr_${srv.serverId || i}_${Date.now()}`,
        name: srv.name || srv.address,
        address: srv.address,
        serverId: srv.serverId,
        lastMessage: 'Synced from PC',
        lastTime: 'now',
    }));
}

function MobileApp() {
    const [screen, setScreen] = useState<MobileScreen>('servers');
    const [servers, setServers] = useState<ServerEntry[]>([]);
    const [activeServer, setActiveServer] = useState<ServerEntry | null>(null);

    // Setup state — Agent + Role selection (like PC "选择角色和 CLI Agent")
    const [selectedAgent, setSelectedAgent] = useState<typeof AGENT_LIST[0] | null>(null);
    const [selectedRole, setSelectedRole] = useState<typeof ROLE_LIST[0] | null>(null);
    const [detectedModel, setDetectedModel] = useState<string>('');
    const [setupLoading, setSetupLoading] = useState(false);

    // Chat state
    const [message, setMessage] = useState('');
    const [chatMessages, setChatMessages] = useState<ChatMsg[]>([]);
    const [loading, setLoading] = useState(false);
    const [sessionId, setSessionId] = useState<string | undefined>(undefined);
    const chatBottomRef = useRef<HTMLDivElement>(null);

    // QR scanner state
    const [scanning, setScanning] = useState(false);
    const [scanError, setScanError] = useState('');
    const scannerRef = useRef<Html5Qrcode | null>(null);

    useEffect(() => {
        chatBottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [chatMessages, loading]);

    // QR scan handler
    const handleQRScan = useCallback((decodedText: string) => {
        try {
            const data: QRPayload = JSON.parse(decodedText);
            if (data.app !== 'echobird' || !data.servers?.length) {
                setScanError('Invalid QR code — not an Echobird config');
                return;
            }
            const newServers = qrToServers(data);
            setServers(prev => {
                const existing = new Map(prev.map(s => [s.serverId || s.id, s]));
                newServers.forEach(s => existing.set(s.serverId || s.id, s));
                return Array.from(existing.values());
            });
            stopScanner();
            setScreen('servers');
        } catch {
            setScanError('Could not parse QR code data');
        }
    }, []);

    const startScanner = useCallback(async () => {
        setScanError('');
        setScanning(true);
        try {
            const scanner = new Html5Qrcode('qr-reader');
            scannerRef.current = scanner;
            await scanner.start(
                { facingMode: 'environment' },
                { fps: 10 },
                (decodedText) => handleQRScan(decodedText),
                () => {}
            );
        } catch (err: any) {
            setScanning(false);
            setScanError(err?.message || 'Camera access denied');
        }
    }, [handleQRScan]);

    const stopScanner = useCallback(() => {
        if (scannerRef.current) {
            scannerRef.current.stop().catch(() => {});
            scannerRef.current = null;
        }
        setScanning(false);
    }, []);

    useEffect(() => {
        if (screen === 'qr') startScanner();
        else stopScanner();
        return () => stopScanner();
    }, [screen]);

    // Open server → go to setup screen (not chat directly)
    const openServer = (server: ServerEntry) => {
        setActiveServer(server);
        setSelectedAgent(null);
        setSelectedRole(null);
        setDetectedModel('');
        setScreen('setup');
    };

    // After selecting Agent, try to detect current model on server
    const selectAgent = async (agent: typeof AGENT_LIST[0]) => {
        setSelectedAgent(agent);
        if (activeServer?.serverId) {
            setSetupLoading(true);
            try {
                await invoke('bridge_ensure_remote', { serverId: activeServer.serverId });
                const result = await invoke<{ modelId: string; modelName: string } | null>(
                    'bridge_get_remote_model',
                    { serverId: activeServer.serverId, agentId: agent.id }
                );
                if (result) {
                    setDetectedModel(result.modelName || result.modelId);
                } else {
                    setDetectedModel('No model configured');
                }
            } catch {
                setDetectedModel('Could not detect model');
            } finally {
                setSetupLoading(false);
            }
        }
    };

    // Enter chat after setup is complete
    const startChat = () => {
        if (!selectedAgent) return;
        setChatMessages([]);
        setSessionId(undefined);
        setScreen('chat');
    };

    // Send message via Tauri bridge_chat_remote
    const sendMessage = useCallback(async () => {
        if (!message.trim() || !activeServer?.serverId || loading || !selectedAgent) return;

        const userMsg = message.trim();
        setMessage('');
        setChatMessages(prev => [...prev, { role: 'user', content: userMsg }]);
        setLoading(true);

        try {
            const result = await invoke<{
                text: string;
                session_id?: string;
                model?: string;
                tokens?: number;
                duration_ms?: number;
            }>('bridge_chat_remote', {
                serverId: activeServer.serverId,
                message: userMsg,
                sessionId: sessionId ?? null,
                pluginId: selectedAgent.id,
                roleId: selectedRole?.id !== 'none' ? selectedRole?.id ?? null : null,
            });

            setChatMessages(prev => [...prev, {
                role: 'ai',
                content: result.text,
                model: result.model,
                tokens: result.tokens,
                duration_ms: result.duration_ms,
            }]);

            if (result.session_id) setSessionId(result.session_id);

            setServers(prev => prev.map(s =>
                s.id === activeServer.id
                    ? { ...s, lastMessage: result.text.slice(0, 60), lastTime: 'now' }
                    : s
            ));
        } catch (err: any) {
            setChatMessages(prev => [...prev, {
                role: 'system',
                content: err?.message || String(err),
            }]);
        } finally {
            setLoading(false);
        }
    }, [message, activeServer, loading, sessionId, selectedAgent, selectedRole]);

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
                        <button className="mobile-icon-btn accent" onClick={() => setScreen('qr')}>
                            <Plus size={20} />
                        </button>
                    </div>

                    {servers.length === 0 ? (
                        <div className="empty-servers">
                            <div className="empty-servers-icon">📱</div>
                            <p className="empty-servers-title">No servers yet</p>
                            <p className="empty-servers-desc">
                                Scan QR code from your PC to sync servers
                            </p>
                            <button className="empty-servers-btn" onClick={() => setScreen('qr')}>
                                Scan QR Code
                            </button>
                        </div>
                    ) : (
                        <div className="server-list">
                            {servers.map(s => (
                                <div key={s.id} className="server-item" onClick={() => openServer(s)}>
                                    <div className="server-avatar">
                                        <span className="server-avatar-fallback">{s.name[0]}</span>
                                    </div>
                                    <div className="server-info">
                                        <div className="server-name">{s.name}</div>
                                        <div className="server-preview">{s.lastMessage}</div>
                                    </div>
                                    <div className="server-time">{s.lastTime}</div>
                                </div>
                            ))}
                        </div>
                    )}
                </div>
            )}

            {/* ===== QR Scanner ===== */}
            {screen === 'qr' && (
                <div className="mobile-screen">
                    <div className="mobile-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('servers')}>
                            <ArrowLeft size={20} />
                        </button>
                        <h2 className="mobile-title">Scan QR Code</h2>
                        <div className="mobile-header-spacer" />
                    </div>
                    <div className="qr-container">
                        <div id="qr-reader" className="qr-reader-box" />
                        {scanError && <p className="qr-error">{scanError}</p>}
                        <p className="qr-hint">
                            Open Echobird on your PC<br />
                            Channels → hover the phone icon<br />
                            Point your camera at the code
                        </p>
                    </div>
                </div>
            )}

            {/* ===== Setup Screen (Agent + Role) ===== */}
            {screen === 'setup' && activeServer && (
                <div className="mobile-screen">
                    <div className="mobile-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('servers')}>
                            <ArrowLeft size={20} />
                        </button>
                        <h2 className="mobile-title">{activeServer.name}</h2>
                        <div className="mobile-header-spacer" />
                    </div>

                    <div className="setup-container">
                        {/* Step 1: Select Agent */}
                        <div className="setup-section">
                            <div className="setup-label">CLI Agent</div>
                            <div className="setup-options">
                                {AGENT_LIST.map(agent => (
                                    <div
                                        key={agent.id}
                                        className={`setup-option ${selectedAgent?.id === agent.id ? 'active' : ''}`}
                                        onClick={() => selectAgent(agent)}
                                    >
                                        <img src={agent.icon} alt="" className="setup-option-icon" onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }} />
                                        <span>{agent.name}</span>
                                    </div>
                                ))}
                            </div>
                        </div>

                        {/* Step 2: Select Role (only show after agent is selected) */}
                        {selectedAgent && (
                            <div className="setup-section">
                                <div className="setup-label">Role</div>
                                <div className="setup-options">
                                    {ROLE_LIST.map(role => (
                                        <div
                                            key={role.id}
                                            className={`setup-option ${selectedRole?.id === role.id ? 'active' : ''}`}
                                            onClick={() => setSelectedRole(role)}
                                        >
                                            <span>{role.name}</span>
                                        </div>
                                    ))}
                                </div>
                            </div>
                        )}

                        {/* Step 3: Model info (auto-detected) */}
                        {selectedAgent && (
                            <div className="setup-section">
                                <div className="setup-label">Model</div>
                                <div className="setup-model-info">
                                    {setupLoading ? (
                                        <Loader2 size={16} className="spin" />
                                    ) : (
                                        <span>{detectedModel || 'Select an agent first'}</span>
                                    )}
                                </div>
                            </div>
                        )}

                        {/* Start Chat button */}
                        {selectedAgent && selectedRole && (
                            <button className="setup-start-btn" onClick={startChat}>
                                Start Chat
                            </button>
                        )}
                    </div>
                </div>
            )}

            {/* ===== Chat ===== */}
            {screen === 'chat' && activeServer && selectedAgent && (
                <div className="mobile-screen">
                    <div className="chat-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('setup')}>
                            <ArrowLeft size={20} />
                        </button>
                        <div className="chat-header-info">
                            <div className="chat-role-name">{activeServer.name}</div>
                            <div className="chat-subtitle">
                                {selectedAgent.name}{selectedRole && selectedRole.id !== 'none' ? ` · ${selectedRole.name}` : ''}
                            </div>
                        </div>
                    </div>

                    <div className="chat-messages">
                        {chatMessages.length === 0 && !loading && (
                            <div className="chat-empty">
                                <p>Start chatting with {selectedAgent.name}</p>
                                <p className="chat-empty-sub">{activeServer.name}{detectedModel ? ` · ${detectedModel}` : ''}</p>
                            </div>
                        )}
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

                    <div className="chat-input-bar">
                        <input
                            className="chat-input"
                            placeholder="Type a message..."
                            value={message}
                            onChange={e => setMessage(e.target.value)}
                            onKeyDown={e => e.key === 'Enter' && sendMessage()}
                            disabled={loading}
                        />
                        <button className="send-btn" onClick={sendMessage} disabled={loading}>
                            {loading ? <Loader2 size={18} className="spin" /> : <Send size={18} />}
                        </button>
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
