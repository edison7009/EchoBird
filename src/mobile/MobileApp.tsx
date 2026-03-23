// MobileApp.tsx — Mobile-only application shell
// Telegram-style: Server List → Chat → Role/Model Bottom Sheets
// Only loaded when platform is Android/iOS (or ?mobile dev flag)

import { useState, useEffect, useRef, useCallback } from 'react';
import { ArrowLeft, Settings, Plus, Send, Loader2, X } from 'lucide-react';
import { Html5Qrcode } from 'html5-qrcode';
import { invoke } from '@tauri-apps/api/core';
import './MobileApp.css';

type MobileScreen = 'servers' | 'qr' | 'chat' | 'settings';

// Agent list — same as PC client (hardcoded)
const AGENT_LIST = [
    { id: 'openclaw', name: 'OpenClaw', icon: '/icons/tools/openclaw.svg' },
    { id: 'claudecode', name: 'Claude Code', icon: '/icons/tools/claudecode.svg' },
    { id: 'zeroclaw', name: 'ZeroClaw', icon: '/icons/tools/zeroclaw.png' },
    { id: 'nanobot', name: 'NanoBot', icon: '/icons/tools/nanobot.png' },
    { id: 'picoclaw', name: 'PicoClaw', icon: '/icons/tools/picoclaw.png' },
    { id: 'hermes', name: 'Hermes Agent', icon: '/icons/tools/hermes.png' },
];

interface ServerEntry {
    id: string;
    name: string;
    address: string;
    serverId?: string;
    agent: string;
    agentIcon: string;
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

// QR payload schema — matches PC side ChannelsMobileQR
interface QRPayload {
    app: 'echobird';
    v: number;
    servers: { name: string; address: string; serverId?: string }[];
}

// Parse QR payload → server entries
function qrToServers(data: QRPayload): ServerEntry[] {
    return data.servers.map((srv, i) => ({
        id: `qr_${srv.serverId || i}_${Date.now()}`,
        name: srv.name || srv.address,
        address: srv.address,
        serverId: srv.serverId,
        agent: AGENT_LIST[0].name,
        agentIcon: AGENT_LIST[0].icon,
        lastMessage: 'Synced from PC',
        lastTime: 'now',
    }));
}

function MobileApp() {
    const [screen, setScreen] = useState<MobileScreen>('servers');
    const [servers, setServers] = useState<ServerEntry[]>([]);
    const [activeServer, setActiveServer] = useState<ServerEntry | null>(null);
    const [message, setMessage] = useState('');
    const [selectedAgent, setSelectedAgent] = useState(AGENT_LIST[0]);
    const [showAgentSheet, setShowAgentSheet] = useState(false);
    const [chatMessages, setChatMessages] = useState<ChatMsg[]>([]);
    const [loading, setLoading] = useState(false);
    const [sessionId, setSessionId] = useState<string | undefined>(undefined);
    const chatBottomRef = useRef<HTMLDivElement>(null);

    // QR scanner state
    const [scanning, setScanning] = useState(false);
    const [scanError, setScanError] = useState('');
    const scannerRef = useRef<Html5Qrcode | null>(null);

    // Auto-scroll chat
    useEffect(() => {
        chatBottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [chatMessages, loading]);

    // Handle successful QR scan
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

    // Start camera QR scanner
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

    const openChat = (server: ServerEntry) => {
        setActiveServer(server);
        setChatMessages([]);
        setSessionId(undefined);
        setShowAgentSheet(false);
        setScreen('chat');
    };

    // Send message via Tauri bridge_chat_remote
    const sendMessage = useCallback(async () => {
        if (!message.trim() || !activeServer?.serverId || loading) return;

        const userMsg = message.trim();
        setMessage('');
        setChatMessages(prev => [...prev, { role: 'user', content: userMsg }]);
        setLoading(true);

        try {
            // Ensure bridge CLI is deployed on remote server
            await invoke('bridge_ensure_remote', { serverId: activeServer.serverId });

            // Send chat via SSH bridge
            const agentEntry = AGENT_LIST.find(a => a.name === selectedAgent.name);
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
                pluginId: agentEntry?.id ?? null,
                roleId: null,
            });

            setChatMessages(prev => [...prev, {
                role: 'ai',
                content: result.text,
                model: result.model,
                tokens: result.tokens,
                duration_ms: result.duration_ms,
            }]);

            if (result.session_id) setSessionId(result.session_id);

            // Update server's last message preview
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
    }, [message, activeServer, loading, sessionId, selectedAgent]);

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
                                <div key={s.id} className="server-item" onClick={() => openChat(s)}>
                                    <div className="server-avatar">
                                        <img src={s.agentIcon} alt={s.agent} onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }} />
                                        <span className="server-avatar-fallback">{s.agent[0]}</span>
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

            {/* ===== Chat ===== */}
            {screen === 'chat' && activeServer && (
                <div className="mobile-screen">
                    <div className="chat-header">
                        <button className="mobile-icon-btn" onClick={() => setScreen('servers')}>
                            <ArrowLeft size={20} />
                        </button>
                        <div className="chat-header-info">
                            <div className="chat-role-name">{activeServer.name}</div>
                            <div className="chat-subtitle" onClick={() => setShowAgentSheet(true)}>
                                {selectedAgent.name} <span className="dropdown-arrow">▾</span>
                            </div>
                        </div>
                    </div>

                    <div className="chat-messages">
                        {chatMessages.length === 0 && !loading && (
                            <div className="chat-empty">
                                <p>Start chatting with {selectedAgent.name}</p>
                                <p className="chat-empty-sub">{activeServer.name}</p>
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

            {/* ===== Agent Selection Sheet ===== */}
            {showAgentSheet && (
                <div className="sheet-overlay" onClick={() => setShowAgentSheet(false)} />
            )}
            <div className={`bottom-sheet ${showAgentSheet ? 'open' : ''}`}>
                <div className="sheet-handle" />
                <div className="sheet-title">Select Agent</div>
                <div className="sheet-content">
                    {AGENT_LIST.map(agent => (
                        <div
                            key={agent.id}
                            className={`sheet-option ${agent.id === selectedAgent.id ? 'active' : ''}`}
                            onClick={() => { setSelectedAgent(agent); setShowAgentSheet(false); setSessionId(undefined); }}
                        >
                            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                                <img src={agent.icon} alt="" style={{ width: 20, height: 20 }} onError={e => { (e.target as HTMLImageElement).style.display = 'none'; }} />
                                {agent.name}
                            </div>
                        </div>
                    ))}
                </div>
            </div>
        </div>
    );
}

export default MobileApp;
