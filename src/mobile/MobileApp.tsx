// MobileApp.tsx — Mobile-only application shell
// Telegram-style: Server List → Chat → Role/Model Bottom Sheets
// Only loaded when platform is Android/iOS (or ?mobile dev flag)

import { useState, useEffect } from 'react';
import { ArrowLeft, Settings, Plus, Send, Loader2, X } from 'lucide-react';
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
}

// Placeholder data — will be replaced by QR scan data
const DEMO_SERVERS: ServerEntry[] = [
    { id: '1', host: '192.168.10.39', user: 'eben', port: 22, agent: 'OpenClaw', agentIcon: '/icons/tools/openclaw.svg', lastMessage: 'Build the login page with React...', lastTime: '12:34' },
    { id: '2', host: 'api.example.com', user: 'dev', port: 22, agent: 'Claude Code', agentIcon: '/icons/tools/claudecode.svg', lastMessage: 'Fixed the CSS alignment bug in...', lastTime: 'Yesterday' },
];

function MobileApp() {
    const { t, locale, setLocale } = useI18n();
    const [screen, setScreen] = useState<MobileScreen>('servers');
    const [servers, setServers] = useState<ServerEntry[]>(DEMO_SERVERS);
    const [activeServer, setActiveServer] = useState<ServerEntry | null>(null);
    const [message, setMessage] = useState('');
    const [roleName, setRoleName] = useState('Backend Architect');
    const [modelName, setModelName] = useState('GPT-4o');
    const [showRoleSheet, setShowRoleSheet] = useState(false);
    const [showModelSheet, setShowModelSheet] = useState(false);
    const [chatMessages, setChatMessages] = useState<{ role: 'user' | 'ai'; content: string }[]>([
        { role: 'user', content: 'Build a REST API with JWT authentication' },
        { role: 'ai', content: 'I\'ll create a secure REST API with JWT authentication. Setting up Express server with `jsonwebtoken`, user model with bcrypt, and auth middleware for protected routes.' },
    ]);

    const openChat = (server: ServerEntry) => {
        setActiveServer(server);
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
                        <div className="qr-frame">
                            <div className="qr-scan-line" />
                        </div>
                        <p className="qr-hint">
                            Open Echobird on your PC<br />
                            Channels → QR button<br />
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
                            <div className="chat-role-name">{roleName} <span className="dropdown-arrow">▾</span></div>
                            <div className="chat-subtitle" onClick={e => { e.stopPropagation(); setShowModelSheet(true); }}>
                                {activeServer.agent} · {modelName} <span className="dropdown-arrow">▾</span>
                            </div>
                        </div>
                    </div>

                    <div className="chat-messages">
                        {chatMessages.map((msg, i) => (
                            <div key={i} className={`chat-bubble ${msg.role}`}>
                                {msg.content}
                            </div>
                        ))}
                    </div>

                    <div className="chat-input-bar">
                        <input
                            className="chat-input"
                            placeholder={t('channel.enterMessage')}
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
                        <h2 className="mobile-title">{t('settings.title')}</h2>
                        <div className="mobile-header-spacer" />
                    </div>
                    <div className="settings-list">
                        <div className="settings-item">
                            <span>{t('settings.language')}</span>
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
                <div className="sheet-title">{t('channel.selectRoleAgent')}</div>
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
