// MobileApp.tsx — Mobile-only application shell
// Telegram-style: Server List → Chat → Role/Model Bottom Sheets
// Only loaded when platform is Android/iOS (or ?mobile dev flag)

import { useState, useEffect, useRef, useCallback } from 'react';
import { ArrowLeft, Settings, Plus, Send, Loader2, X } from 'lucide-react';
import { Html5Qrcode } from 'html5-qrcode';
import { useI18n } from '../hooks/useI18n';
import { ChannelsProvider } from '../pages/Channels';
import './MobileApp.css';

type MobileScreen = 'servers' | 'qr' | 'chat' | 'settings';

interface ServerEntry {
    id: string;
    host: string;
    user: string;
    port: number;
    agent: string;
    agentIcon: string;
    lastMessage: string;
    lastTime: string;
    serverId?: string;
}

// QR payload schema — matches PC side ChannelsMobileQR
interface QRPayload {
    app: 'echobird';
    v: number;
    servers: { name: string; address: string; serverId?: string }[];
    agents: { channelId: number; agent: string }[];
    roles: { channelId: number; roleId: string; roleName: string }[];
}

// Parse QR payload → server entries
function qrToServers(data: QRPayload): ServerEntry[] {
    return data.servers.map((srv, i) => {
        const parts = srv.name || srv.address || '';
        const [user, host] = parts.includes('@') ? parts.split('@') : ['', parts];
        return {
            id: `qr_${i}_${Date.now()}`,
            host: host || srv.address,
            user: user || 'user',
            port: 22,
            agent: data.agents.find(a => a.channelId === i + 2)?.agent || 'OpenClaw',
            agentIcon: '/icons/tools/openclaw.svg',
            lastMessage: 'Synced from PC',
            lastTime: 'now',
            serverId: srv.serverId,
        };
    });
}

function MobileApp() {
    const { t, locale, setLocale } = useI18n();
    const [screen, setScreen] = useState<MobileScreen>('servers');
    const [servers, setServers] = useState<ServerEntry[]>([]);
    const [activeServer, setActiveServer] = useState<ServerEntry | null>(null);
    const [message, setMessage] = useState('');
    const [roleName, setRoleName] = useState('');
    const [modelName, setModelName] = useState('');
    const [showRoleSheet, setShowRoleSheet] = useState(false);
    const [showModelSheet, setShowModelSheet] = useState(false);
    const [chatMessages, setChatMessages] = useState<{ role: 'user' | 'ai'; content: string }[]>([]);

    // QR scanner state
    const [scanning, setScanning] = useState(false);
    const [scanError, setScanError] = useState('');
    const scannerRef = useRef<Html5Qrcode | null>(null);

    // Handle successful QR scan
    const handleQRScan = useCallback((decodedText: string) => {
        try {
            const data: QRPayload = JSON.parse(decodedText);
            if (data.app !== 'echobird' || !data.servers?.length) {
                setScanError('Invalid QR code — not an Echobird config');
                return;
            }
            // Convert QR data to server entries
            const newServers = qrToServers(data);
            setServers(prev => {
                // Merge: replace existing by serverId, add new ones
                const existing = new Map(prev.map(s => [s.serverId || s.id, s]));
                newServers.forEach(s => existing.set(s.serverId || s.id, s));
                return Array.from(existing.values());
            });
            // Stop scanner and go to server list
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
                { fps: 10, qrbox: { width: 250, height: 250 } },
                (decodedText) => handleQRScan(decodedText),
                () => {} // ignore scan failures (normal while pointing)
            );
        } catch (err: any) {
            setScanning(false);
            setScanError(err?.message || 'Camera access denied');
        }
    }, [handleQRScan]);

    // Stop scanner
    const stopScanner = useCallback(() => {
        if (scannerRef.current) {
            scannerRef.current.stop().catch(() => {});
            scannerRef.current = null;
        }
        setScanning(false);
    }, []);

    // Auto-start scanner when entering QR screen
    useEffect(() => {
        if (screen === 'qr') {
            startScanner();
        } else {
            stopScanner();
        }
        return () => stopScanner();
    }, [screen]);

    const openChat = (server: ServerEntry) => {
        setActiveServer(server);
        setChatMessages([]);
        setScreen('chat');
    };

    const sendMessage = () => {
        if (!message.trim()) return;
        setChatMessages(prev => [...prev, { role: 'user', content: message }]);
        setMessage('');
        // TODO: Wire to bridge_chat_remote via SSH
    };

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
                                        <div className="server-name">{s.user}@{s.host}</div>
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
                        {/* Camera preview — html5-qrcode mounts here */}
                        <div id="qr-reader" className="qr-reader-box" />

                        {scanError && (
                            <p className="qr-error">{scanError}</p>
                        )}

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
                        <div className="chat-header-info" onClick={() => setShowRoleSheet(true)}>
                            <div className="chat-role-name">{roleName || activeServer.agent} <span className="dropdown-arrow">▾</span></div>
                            <div className="chat-subtitle" onClick={e => { e.stopPropagation(); setShowModelSheet(true); }}>
                                {activeServer.user}@{activeServer.host} · {modelName || 'Select Model'} <span className="dropdown-arrow">▾</span>
                            </div>
                        </div>
                    </div>

                    <div className="chat-messages">
                        {chatMessages.length === 0 && (
                            <div className="chat-empty">
                                <p>Start chatting with {activeServer.agent}</p>
                                <p className="chat-empty-sub">{activeServer.user}@{activeServer.host}</p>
                            </div>
                        )}
                        {chatMessages.map((msg, i) => (
                            <div key={i} className={`chat-bubble ${msg.role}`}>
                                {msg.content}
                            </div>
                        ))}
                    </div>

                    <div className="chat-input-bar">
                        <input
                            className="chat-input"
                            placeholder="Type a message..."
                            value={message}
                            onChange={e => setMessage(e.target.value)}
                            onKeyDown={e => e.key === 'Enter' && sendMessage()}
                        />
                        <button className="send-btn" onClick={sendMessage}>
                            <Send size={18} />
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
                            <span>Language</span>
                            <select
                                value={locale}
                                onChange={e => setLocale(e.target.value)}
                                className="settings-select"
                            >
                                <option value="en">English</option>
                                <option value="zh-Hans">简体中文</option>
                                <option value="zh-Hant">繁體中文</option>
                                <option value="ja">日本語</option>
                                <option value="ko">한국어</option>
                                <option value="de">Deutsch</option>
                                <option value="fr">Français</option>
                                <option value="es">Español</option>
                            </select>
                        </div>
                        <div className="settings-item">
                            <span>Version</span>
                            <span className="settings-value">3.2.7</span>
                        </div>
                    </div>
                </div>
            )}

            {/* ===== Bottom Sheets ===== */}
            {(showRoleSheet || showModelSheet) && (
                <div className="sheet-overlay" onClick={() => { setShowRoleSheet(false); setShowModelSheet(false); }} />
            )}

            {/* Role Sheet */}
            <div className={`bottom-sheet ${showRoleSheet ? 'open' : ''}`}>
                <div className="sheet-handle" />
                <div className="sheet-title">Select Role</div>
                <div className="sheet-content">
                    {['Backend Architect', 'Data Engineer', 'DevOps Engineer', 'AI Engineer', 'Frontend Dev', 'Code Auditor'].map(role => (
                        <div
                            key={role}
                            className={`sheet-option ${role === roleName ? 'active' : ''}`}
                            onClick={() => { setRoleName(role); setShowRoleSheet(false); }}
                        >
                            {role}
                        </div>
                    ))}
                </div>
            </div>

            {/* Model Sheet */}
            <div className={`bottom-sheet ${showModelSheet ? 'open' : ''}`}>
                <div className="sheet-handle" />
                <div className="sheet-title">Select Model</div>
                <div className="sheet-content">
                    {['GPT-4o', 'Claude Sonnet 4', 'Gemini 2.5 Pro', 'DeepSeek R1', 'Qwen3.5 27B'].map(model => (
                        <div
                            key={model}
                            className={`sheet-option ${model === modelName ? 'active' : ''}`}
                            onClick={() => { setModelName(model); setShowModelSheet(false); }}
                        >
                            {model}
                        </div>
                    ))}
                </div>
            </div>
        </div>
    );
}

export default MobileApp;
