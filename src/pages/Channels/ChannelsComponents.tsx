// ChannelsComponents.tsx — Panel, RoleSelector, MobileSync (extracted from Channels.tsx)
import React, { useState, useEffect } from 'react';
import { ChevronDown } from 'lucide-react';
import { MobileQRPopup } from '../../components/MobileQRPopup';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import { AGENT_LIST } from '../../api/agentList';
import { useChannels } from './context';

// ===== ChannelsPanel — right-side channel list (rendered in aside) =====
export function ChannelsPanel() {
    const { channels, activeId, selectChannel, allBridgeStatus, allActiveAgents, allBridgeLoading, allSelectedRoles, allBridgeHasNew } = useChannels();
    const { t } = useI18n();

    return (
        <>
            {/* Header */}
            <div className="p-2 bg-transparent">
                <span className="text-xs font-bold tracking-wider text-cyber-accent font-mono">{t('mother.servers')}</span>
            </div>

            {/* Channel list */}
            <div className="flex-1 p-2 overflow-y-auto slim-scroll">
                <div className="space-y-2">
                    {channels.map(ch => {
                        const isActive = activeId === ch.id;

                        const chBridgeStatus = allBridgeStatus[ch.id] || 'standby';
                        const isLinked = chBridgeStatus === 'connected';
                        const isBridgeConnecting = chBridgeStatus === 'connecting';
                        const isError = chBridgeStatus === 'disconnected';
                        const isTyping = allBridgeLoading[ch.id] || false;
                        const hasNew = allBridgeHasNew[ch.id] && !isActive;

                        return (
                            <div
                                key={ch.id}
                                onClick={() => selectChannel(ch.id)}
                                className={`w-full text-left p-3 transition-all font-mono rounded-card cursor-pointer ${isActive
                                    ? 'bg-cyber-accent/10'
                                    : 'bg-black/30 hover:bg-white/5'
                                    }`}
                            >
                                <div className="flex items-center gap-3">
                                    {(() => {
                                    const selectedAgent = allActiveAgents[ch.id];
                                        const agent = selectedAgent ? AGENT_LIST.find(a => a.name === selectedAgent) : null;
                                        if (agent) {
                                            return <img src={agent.icon} alt={agent.name} className="w-9 h-9 flex-shrink-0" />;
                                        }
                                        return (
                                            <div className="w-9 h-9 flex-shrink-0 rounded-lg bg-cyber-border/30 flex items-center justify-center">
                                                <span className="text-cyber-text-muted/40 text-lg font-bold">?</span>
                                            </div>
                                        );
                                    })()}
                                    <div className="flex-1 min-w-0">
                                        <div className="flex items-center gap-1 mb-1">
                                            <span className={`text-sm font-bold whitespace-nowrap truncate ${isActive ? 'text-cyber-accent' : 'text-cyber-accent/90'}`}>
                                                {ch.name || (ch.address?.startsWith('127.0.0.1') || ch.address === 'localhost'
                                                    ? `${t('mother.local')} (127.0.0.1)` : ch.address)}
                                            </span>
                                            <div className={`ml-auto w-2 h-2 rounded-full flex-shrink-0 ${hasNew ? 'bg-red-500 animate-pulse' : isLinked ? 'bg-cyber-accent animate-pulse' : isBridgeConnecting ? 'bg-yellow-400 animate-pulse' : isError ? 'bg-red-400' : 'bg-cyber-text-muted/50'}`} />
                                        </div>
                                        <div className="flex items-center gap-1.5 overflow-hidden">
                                            <span className={`text-xs tracking-wide whitespace-nowrap flex-shrink-0 ${isTyping ? 'text-cyber-accent' : isLinked ? 'text-cyber-accent' : isBridgeConnecting ? 'text-yellow-400' : isError ? 'text-red-400' : 'text-cyber-text-muted/70'}`}>
                                                [{isTyping ? t('common.inputting') : isLinked ? t('channel.linked') : isBridgeConnecting ? t('channel.connecting') : isError ? t('channel.failed') : t('channel.standby')}]
                                            </span>
                                            {(allSelectedRoles[ch.id]?.name || allActiveAgents[ch.id]) && (
                                                <span className={`text-xs min-w-0 truncate ${isTyping ? 'text-cyber-accent' : isLinked ? 'text-cyber-accent' : isBridgeConnecting ? 'text-yellow-400' : isError ? 'text-red-400' : 'text-cyber-text-muted/70'}`}>
                                                    {allSelectedRoles[ch.id]?.name || allActiveAgents[ch.id]}
                                                </span>
                                            )}
                                        </div>
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                </div>
            </div>
        </>
    );
}

// ===== ChannelsRoleSelector — title bar widget (styled like MiniSelect) =====
export function ChannelsRoleSelector() {
    const { channels, activeId, allActiveAgents, allSelectedRoles } = useChannels();
    const { t } = useI18n();
    const channelKey = activeId ?? '';
    const activeChannel = channels.find(c => c.id === activeId);
    const selectedAgent = (allActiveAgents as Record<string, string>)[channelKey] || '';
    const agent = selectedAgent ? AGENT_LIST.find(a => a.name === selectedAgent) : null;
    const selectedRoleForChannel = (allSelectedRoles as Record<string, { id: string; name: string; filePath: string }>)[channelKey] || { id: '', name: '', filePath: '' };
    const hasRole = selectedRoleForChannel && selectedRoleForChannel.id;

    // Dispatch event to open AgentRolePicker in ChannelsMain
    const openPicker = () => window.dispatchEvent(new CustomEvent('open-role-picker'));

    if (!activeChannel) return null;

    const label = hasRole
        ? selectedRoleForChannel.name
        : selectedAgent || t('channel.selectRoleAgent');

    return (
        <div className="relative">
            <button
                type="button"
                onClick={openPicker}
                className={`w-full min-w-[90px] bg-black border px-3 py-1.5 outline-none cursor-pointer flex items-center justify-center transition-colors text-xs font-mono rounded-button ${
                    (hasRole || selectedAgent)
                        ? 'border-cyber-accent/40 hover:border-cyber-accent/70'
                        : 'border-cyber-border hover:border-cyber-accent/50'
                }`}
            >
                <span className="truncate text-cyber-text">{label}</span>
                <ChevronDown size={12} className="flex-shrink-0 ml-1 text-cyber-accent" />
            </button>
        </div>
    );
}

// ===== ChannelsMobileSync — clipboard-based config sync for mobile =====
// Generates "eb:" + base64(JSON) config string. Delegates UI to MobileQRPopup.
export function ChannelsMobileSync() {
    const [configCode, setConfigCode] = useState('');

    useEffect(() => {
        (async () => {
            try {
                // Load SSH servers and decrypt passwords
                const sshServers = await api.loadSSHServers();
                const decryptedSSH = await Promise.all(
                    sshServers.map(async (s) => ({
                        h: s.host,
                        o: s.port,
                        u: s.username,
                        p: s.password?.startsWith('enc:v1:')
                            ? await api.decryptSSHPassword(s.password)
                            : (s.password || ''),
                        n: s.alias || '',
                    }))
                );

                // Load user models and decrypt API keys
                const allModels = await api.getModels();
                const userModels = allModels.filter(m =>
                    m.modelType !== 'LOCAL' && m.modelType !== 'DEMO' && m.internalId !== 'local-server'
                );
                const decryptedModels = await Promise.all(
                    userModels.map(async (m) => ({
                        n: m.name,
                        i: m.modelId || m.name,
                        b: m.baseUrl,
                        k: m.apiKey?.startsWith('enc:v1:')
                            ? await api.decryptSSHPassword(m.apiKey)
                            : (m.apiKey || ''),
                        x: m.anthropicUrl || '',
                    }))
                );

                const json = JSON.stringify({ a: 'echobird', v: 2, s: decryptedSSH, m: decryptedModels });
                const encoded = 'eb:' + btoa(unescape(encodeURIComponent(json)));
                setConfigCode(encoded);
            } catch (e) {
                console.error('[MobileSync] Failed to build config code:', e);
            }
        })();
    }, []);

    return <MobileQRPopup configCode={configCode} />;
}
