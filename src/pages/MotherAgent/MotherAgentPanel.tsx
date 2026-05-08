import { useState } from 'react';
import { X, ChevronDown } from 'lucide-react';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import { useMotherAgent } from './context';

// ===== Right Panel (aside area) — SERVERS =====
export function MotherAgentPanel() {
    const { setChatInput, chatInputRef, sshServers, addSSHServer, removeSSHServer, selectedServerId, selectServer, isProcessing } = useMotherAgent();
    const confirm = useConfirm();
    const { t } = useI18n();

    const [panelTab, setPanelTab] = useState<'servers' | 'guide'>('servers');
    const [showSSHModal, setShowSSHModal] = useState(false);
    const [sshForm, setSSHForm] = useState({ host: '', port: '22', username: '', password: '', alias: '', showPassword: false });
    const [sshTestResult, setSSHTestResult] = useState<{ success: boolean; message: string } | null>(null);
    const [sshTesting, setSSHTesting] = useState(false);
    const [expandedGuide, setExpandedGuide] = useState<string | null>(null);

    const handleSSHTest = async () => {
        if (!sshForm.host.trim() || !sshForm.username.trim()) return;
        setSSHTesting(true);
        setSSHTestResult(null);
        try {
            // Step 1: Test SSH connectivity
            const result = await api.sshTestConnection(
                sshForm.host.trim(),
                parseInt(sshForm.port) || 22,
                sshForm.username.trim(),
                sshForm.password,
            );
            if (!result.success) {
                setSSHTestResult(result);
                return;
            }

            setSSHTestResult(result);
        } catch (e) {
            setSSHTestResult({ success: false, message: String(e) });
        } finally {
            setSSHTesting(false);
        }
    };



    return (
        <>
            {/* Header with tabs */}
            <div className="p-2 flex items-center justify-between bg-transparent">
                <div className="flex gap-1">
                    <button
                        onClick={() => setPanelTab('servers')}
                        className={`px-3.5 py-2 text-[14px] font-semibold rounded transition-colors ${panelTab === 'servers'
                            ? 'bg-cyber-elevated text-cyber-text'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('mother.servers')}
                    </button>
                    <button
                        onClick={() => setPanelTab('guide')}
                        className={`px-3.5 py-2 text-[14px] font-semibold rounded transition-colors ${panelTab === 'guide'
                            ? 'bg-cyber-elevated text-cyber-text'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                            }`}
                    >
                        {t('mother.sshGuide')}
                    </button>
                </div>
            </div>

            {/* SSH Add Modal */}
            {showSSHModal && (
                <div
                    className="fixed inset-0 z-[9998] flex items-center justify-center"
                    onKeyDown={e => { if (e.key === 'Escape') setShowSSHModal(false); }}
                >
                    {/* Backdrop */}
                    <div
                        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                        onClick={() => setShowSSHModal(false)}
                    />

                    <div
                        className="relative w-[400px] max-w-[90vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden"
                        onClick={e => e.stopPropagation()}
                    >
                        {/* Top accent line */}
                        <div className="h-px w-full bg-cyber-border" />

                        {/* Header */}
                        <div className="px-6 pt-5 pb-4 flex items-center justify-between">
                            <div className="flex items-center gap-2">
                                <span className="text-cyber-text font-mono text-sm opacity-60">&gt;_</span>
                                <span className="text-base font-bold text-cyber-text">{t('mother.addServer')}</span>
                            </div>
                            <button
                                onClick={() => setShowSSHModal(false)}
                                className="text-cyber-text-secondary hover:text-cyber-text transition-colors"
                            >
                                <X size={18} />
                            </button>
                        </div>

                        {/* Form */}
                        <div className="px-5 pb-5">
                            <div className="space-y-4">
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.hostIp')}</label>
                                    <input
                                        type="text"
                                        placeholder={t('mother.hostPlaceholder')}
                                        value={sshForm.host}
                                        onChange={e => setSSHForm(f => ({ ...f, host: e.target.value }))}
                                        className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                                        autoFocus
                                    />
                                </div>
                                {/* Display Name — optional alias */}
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">
                                        {t('mother.displayName')} <span className="opacity-50">({t('mother.optional')})</span>
                                    </label>
                                    <input
                                        type="text"
                                        placeholder={t('mother.displayNamePlaceholder')}
                                        value={sshForm.alias}
                                        onChange={e => setSSHForm(f => ({ ...f, alias: e.target.value }))}
                                        className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                                    />
                                </div>
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.port')}</label>
                                    <input
                                        type="number"
                                        placeholder="22"
                                        value={sshForm.port}
                                        onChange={e => setSSHForm(f => ({ ...f, port: e.target.value }))}
                                        className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button no-spinner"
                                    />
                                </div>
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.username')}</label>
                                    <input
                                        type="text"
                                        placeholder={t('mother.userPlaceholder')}
                                        value={sshForm.username}
                                        onChange={e => setSSHForm(f => ({ ...f, username: e.target.value }))}
                                        className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                                    />
                                </div>
                                <div>
                                    <label className="block text-xs text-cyber-text-secondary mb-1">{t('mother.passwordKey')}</label>
                                    <div className="relative">
                                        <input
                                            type="text"
                                            placeholder={t('mother.passwordPlaceholder')}
                                            value={sshForm.password.startsWith('enc:v1:') ? '•••••••••••••••' : sshForm.password}
                                            onChange={e => setSSHForm(f => ({ ...f, password: e.target.value }))}
                                            className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 pr-8 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                                            readOnly={sshForm.password.startsWith('enc:v1:')}
                                        />
                                        <button
                                            type="button"
                                            disabled={!sshForm.password}
                                            onClick={async () => {
                                                if (!sshForm.password) return;
                                                if (sshForm.password.startsWith('enc:v1:')) {
                                                    // Decrypt
                                                    try {
                                                        const plain = await api.decryptSSHPassword(sshForm.password);
                                                        setSSHForm(f => ({ ...f, password: plain || '' }));
                                                    } catch {
                                                        setSSHForm(f => ({ ...f, password: '' }));
                                                    }
                                                } else {
                                                    // Encrypt
                                                    try {
                                                        const encrypted = await api.encryptSSHPassword(sshForm.password);
                                                        setSSHForm(f => ({ ...f, password: encrypted }));
                                                    } catch {
                                                        // stay plaintext on failure
                                                    }
                                                }
                                            }}
                                            className={`absolute right-2 top-1/2 -translate-y-1/2 transition-colors hover:opacity-80 ${!sshForm.password ? 'opacity-20' : ''}`}
                                        >
                                            {sshForm.password.startsWith('enc:v1:') ? (
                                                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-cyber-text">
                                                    <rect x="3" y="11" width="18" height="11" rx="2" />
                                                    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                                                </svg>
                                            ) : (
                                                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-cyber-text-muted">
                                                    <rect x="3" y="11" width="18" height="11" rx="2" />
                                                    <path d="M7 11V7a5 5 0 0 1 9.9-1" />
                                                </svg>
                                            )}
                                        </button>
                                    </div>
                                    <div className="min-h-[36px] mt-1">
                                        {sshForm.password.startsWith('enc:v1:') && (
                                            <div className="text-xs leading-tight text-cyber-text/60">
                                                {t('mother.encrypted')}
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </div>
                        </div>

                        {/* Test connection button */}
                        <div className="px-5 pb-4">
                            <button
                                onClick={handleSSHTest}
                                disabled={sshTesting || !sshForm.host.trim() || !sshForm.username.trim()}
                                className="w-full py-2 text-xs font-mono font-bold tracking-wider border border-cyber-border/40 text-cyber-text rounded-button hover:bg-cyber-text/10 transition-all disabled:opacity-30 disabled:cursor-not-allowed"
                            >
                                {sshTesting ? t('mother.testing') : t('mother.testConnection')}
                            </button>
                            {sshTestResult && !sshTesting && (
                                <div className={`text-[11px] font-mono mt-2 px-3 py-2 rounded border ${sshTestResult.success
                                    ? 'border-green-500/30 text-green-400 bg-green-500/5'
                                    : 'border-red-500/30 text-red-400 bg-red-500/5'
                                    }`}>
                                    {sshTestResult.message}
                                </div>
                            )}
                        </div>

                        {/* Footer buttons */}
                        <div className="flex border-t border-cyber-border">
                            <button
                                onClick={() => { setShowSSHModal(false); setSSHTestResult(null); }}
                                className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated transition-all border-r border-cyber-border"
                            >
                                {t('mother.cancel')}
                            </button>
                            <button
                                onClick={async () => {
                                    if (!sshForm.host.trim()) return;
                                    const host = sshForm.host.trim();
                                    const username = sshForm.username.trim();
                                    // Auto-overwrite: remove existing server with same host + username
                                    const existing = sshServers.find(s => s.host === host && s.username === username);
                                    if (existing) await removeSSHServer(existing.id);
                                    const newServer = {
                                        id: Date.now().toString(),
                                        host,
                                        port: sshForm.port || '22',
                                        username,
                                        password: sshForm.password,
                                        alias: sshForm.alias.trim() || undefined,
                                    };
                                    await addSSHServer(newServer);
                                    setSSHForm({ host: '', port: '22', username: '', password: '', alias: '', showPassword: false });
                                    setSSHTestResult(null);
                                    setShowSSHModal(false);
                                }}
                                className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text hover:bg-cyber-text/10 transition-all"
                            >
                                {t('mother.addServerBtn')}
                            </button>
                        </div>
                    </div>
                </div >
            )
            }

            {/* Content */}
            <div className="flex-1 p-2 overflow-y-auto slim-scroll">
                {panelTab === 'servers' ? (
                    /* ── SERVERS tab ── */
                    <div className="space-y-2">
                        {/* Server list */}
                        {/* Local server — always first */}
                        <div
                            onClick={() => !isProcessing && selectServer('local')}
                            className={`p-3 rounded transition-colors select-none flex items-center border bg-cyber-surface ${isProcessing && selectedServerId !== 'local' ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'} ${selectedServerId === 'local'
                                ? 'border-cyber-accent'
                                : 'border-transparent hover:bg-cyber-elevated'
                                }`}
                        >
                            <div className="mr-3">
                                <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center transition-all ${selectedServerId === 'local' ? 'border-cyber-border' : 'border-cyber-text-muted/30'}`}>
                                    {selectedServerId === 'local' && <div className="w-2 h-2 rounded-full bg-cyber-accent" />}
                                </div>
                            </div>
                            <div className="flex-1 min-w-0">
                                <div className="text-xs text-cyber-text-secondary mb-0.5 tracking-widest uppercase font-mono">{t('mother.local')}</div>
                                <div className="text-sm font-bold truncate text-cyber-text font-mono">127.0.0.1</div>
                            </div>
                        </div>
                        {/* SSH servers */}
                        {sshServers.map(server => (
                            <div
                                key={server.id}
                                onClick={() => !isProcessing && selectServer(server.id)}
                                className={`p-3 rounded transition-colors select-none flex items-center border bg-cyber-surface ${isProcessing && selectedServerId !== server.id ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'} ${selectedServerId === server.id
                                    ? 'border-cyber-accent'
                                    : 'border-transparent hover:bg-cyber-elevated'
                                    }`}
                            >
                                <div className="mr-3">
                                    <div className={`w-4 h-4 rounded-full border-2 flex items-center justify-center transition-all ${selectedServerId === server.id ? 'border-cyber-border' : 'border-cyber-text-muted/30'}`}>
                                        {selectedServerId === server.id && <div className="w-2 h-2 rounded-full bg-cyber-accent" />}
                                    </div>
                                </div>
                                <div className="flex-1 min-w-0">
                                    <div className="flex items-center gap-1 mb-0.5">
                                        <span className="text-xs text-cyber-text-secondary tracking-widest uppercase font-mono truncate flex-1 min-w-0">{server.alias || t('mother.local')}</span>
                                        <button
                                            onClick={async (e) => {
                                                e.stopPropagation();
                                                const ok = await confirm({
                                                    title: t('mother.deleteServerTitle'),
                                                    message: t('mother.deleteServerMsg'),
                                                    confirmText: t('btn.delete'),
                                                    cancelText: t('btn.cancel'),
                                                    type: 'danger'
                                                });
                                                if (ok) removeSSHServer(server.id);
                                            }}
                                            className="text-xs font-mono text-cyber-text-muted/50 hover:text-red-500 transition-colors flex-shrink-0"
                                        >
                                            [{t('btn.delete')}]
                                        </button>
                                        <button
                                            onClick={async (e) => {
                                                e.stopPropagation();
                                                // Load encrypted password from backend
                                                let savedPassword = '';
                                                try {
                                                    const servers = await api.loadSSHServers();
                                                    const saved = servers.find(s => s.id === server.id);
                                                    if (saved?.password) savedPassword = saved.password;
                                                } catch { }
                                                setSSHForm({
                                                    host: server.host,
                                                    port: server.port,
                                                    username: server.username,
                                                    password: savedPassword,
                                                    alias: server.alias || '',
                                                    showPassword: false,
                                                });
                                                setSSHTestResult(null);
                                                setShowSSHModal(true);
                                            }}
                                            className="text-xs font-mono text-cyber-text-muted/50 hover:text-cyber-text transition-colors flex-shrink-0"
                                        >
                                            [{t('btn.edit')}]
                                        </button>
                                    </div>
                                    <div className="text-sm font-bold truncate text-cyber-text font-mono">{server.username ? `${server.username}@` : ''}{server.host}{server.port !== '22' ? `:${server.port}` : ''}</div>
                                </div>
                            </div>
                        ))}
                        {/* Add button card */}
                        <button
                            onClick={() => {
                                setSSHForm({ host: '', port: '22', username: '', password: '', alias: '', showPassword: false });
                                setSSHTestResult(null);
                                setShowSSHModal(true);
                            }}
                            className="w-full p-4 border border-dashed border-cyber-border/30 rounded hover:border-cyber-border/60 hover:bg-cyber-text/5 transition-all text-cyber-text/60 hover:text-cyber-text text-[14px] font-semibold"
                        >
                            + {t('mother.addServer')}
                        </button>
                    </div>
                ) : (
                    /* ── SSH GUIDE tab (accordion) ── */
                    <div className="space-y-1.5 text-[13px]">
                        {[
                            {
                                id: 'cloud', label: 'Cloud Server', content: (
                                    <>
                                        <p className="text-cyber-text-muted/80">{t('ssh.cloudDesc')}</p>
                                        <div className="mt-2 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.usernameHint')}</span> — {t('ssh.cloudUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.passwordHint')}</span> — {t('ssh.cloudPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.ipHint')}</span> — {t('ssh.cloudIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'windows', label: 'Windows', content: (
                                    <>
                                        <p className="text-cyber-text-muted/70 leading-relaxed">{t('ssh.winNote')}</p>
                                    </>
                                )
                            },
                            {
                                id: 'macos', label: 'macOS', content: (
                                    <>
                                        <p className="text-cyber-text-muted/70">{t('ssh.macStep')}</p>
                                        <p className="text-cyber-text-muted/50 mt-0.5">{t('ssh.macOr')} <span className="text-cyber-text">sudo systemsetup -setremotelogin on</span></p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.usernameHint')}</span> — {t('ssh.macUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.passwordHint')}</span> — {t('ssh.macPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.ipHint')}</span> — {t('ssh.macIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'linux', label: 'Linux', content: (
                                    <>
                                        <p className="text-cyber-text">sudo apt install openssh-server</p>
                                        <p className="text-cyber-text">sudo systemctl enable --now ssh</p>
                                        <p className="text-cyber-text-muted/50 text-xs">{t('ssh.linuxNote')}</p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.usernameHint')}</span> — {t('ssh.linuxUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.passwordHint')}</span> — {t('ssh.linuxPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.ipHint')}</span> — {t('ssh.linuxIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'android', label: 'Android (Termux)', content: (
                                    <>
                                        <p className="text-cyber-text">pkg install openssh && sshd</p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.usernameHint')}</span> — {t('ssh.termuxUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.passwordHint')}</span> — {t('ssh.termuxPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.ipHint')}</span> — {t('ssh.termuxIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.portHint')}</span> — 8022</p>
                                        </div>
                                    </>
                                )
                            },
                            {
                                id: 'ios', label: 'iOS (iSH)', content: (
                                    <>
                                        <p className="text-cyber-text">apk add openssh</p>
                                        <p className="text-cyber-text">ssh-keygen -A && /usr/sbin/sshd</p>
                                        <div className="mt-2 pt-1 border-t border-cyber-border/20 text-xs space-y-0.5">
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.usernameHint')}</span> — {t('ssh.ishUsername')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.passwordHint')}</span> — {t('ssh.ishPassword')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.ipHint')}</span> — {t('ssh.ishIp')}</p>
                                            <p className="text-cyber-text-muted/70">• <span className="text-cyber-text">{t('ssh.portHint')}</span> — 22</p>
                                        </div>
                                    </>
                                )
                            },
                        ].map(section => (
                            <div key={section.id} className="border border-cyber-border/20 rounded overflow-hidden">
                                <button
                                    onClick={() => setExpandedGuide(prev => prev === section.id ? null : section.id)}
                                    className="w-full px-3 py-2.5 flex items-center justify-between bg-cyber-text/5 hover:bg-cyber-text/10 transition-colors"
                                >
                                    <span className="text-cyber-text font-semibold text-[14px]">{section.label}</span>
                                    <ChevronDown size={14} className={`text-cyber-text/60 transition-transform ${expandedGuide === section.id ? 'rotate-180' : ''}`} />
                                </button>
                                {expandedGuide === section.id && (
                                    <div className="px-3 py-2 space-y-1">
                                        {section.content}
                                    </div>
                                )}
                            </div>
                        ))}
                    </div>
                )}
            </div>
        </>
    );
}
