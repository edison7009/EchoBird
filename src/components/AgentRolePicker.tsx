// AgentRolePicker — Combined role + agent selector modal
// Vertical image cards with hover slide-up text effect + category filter + agent tool selector
// Roles and categories loaded dynamically from CDN (echobird.ai/roles/)
import React, { useState, useEffect } from 'react';
import { AlertCircle, ArrowLeft, Check, Loader2, X } from 'lucide-react';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import type { RoleCategory, RoleEntry, AgentStatus } from '../api/tauri';

// ── Agent tools ──
const AGENT_TOOLS = [
    { id: 'openclaw', name: 'OpenClaw', icon: '/icons/tools/openclaw.svg', enabled: true },
    { id: 'claudecode', name: 'Claude Code', icon: '/icons/tools/claudecode.svg', enabled: true },
    { id: 'zeroclaw', name: 'ZeroClaw', icon: '/icons/tools/zeroclaw.png', enabled: true },
    { id: 'nanobot', name: 'NanoBot', icon: '/icons/tools/nanobot.png', enabled: true },
    { id: 'picoclaw', name: 'PicoClaw', icon: '/icons/tools/picoclaw.png', enabled: true },
    { id: 'hermes', name: 'Hermes Agent', icon: '/icons/tools/hermes.png', enabled: true },
];

// ── Component ──

interface AgentRolePickerProps {
    isOpen: boolean;
    onClose: () => void;
    selectedRole: string | null;
    onSelectRole: (roleId: string, roleName: string, filePath: string) => void;
    selectedAgent: string;
    onSelectAgent: (agentName: string) => void;
    isRemote?: boolean;
    remoteServerId?: string;
}

export const AgentRolePicker: React.FC<AgentRolePickerProps> = ({
    isOpen,
    onClose,
    selectedRole,
    onSelectRole,
    selectedAgent,
    onSelectAgent,
    isRemote = false,
    remoteServerId,
}) => {
    const { t, locale } = useI18n();
    const [localSelected, setLocalSelected] = useState<string | null>(selectedRole);
    const [activeCat, setActiveCat] = useState('all');
    const [categories, setCategories] = useState<RoleCategory[]>([]);
    const [roles, setRoles] = useState<RoleEntry[]>([]);
    const [allLabel, setAllLabel] = useState('All');
    const [loading, setLoading] = useState(false);
    const [agentStatuses, setAgentStatuses] = useState<AgentStatus[]>([]);
    const [detecting, setDetecting] = useState(false);
    const [connectionError, setConnectionError] = useState(false);

    // Sync localSelected when prop changes (e.g. switching channels)
    useEffect(() => {
        setLocalSelected(selectedRole);
    }, [selectedRole]);

    // Load roles + detect agents when modal opens
    useEffect(() => {
        if (!isOpen) return;
        let cancelled = false;
        setConnectionError(false);

        // Scan roles
        setLoading(true);
        api.scanRoles(locale).then(result => {
            if (cancelled) return;
            setCategories(result.categories);
            setRoles(result.roles);
            setAllLabel(result.allLabel || 'All');
            setLoading(false);
        }).catch(() => {
            if (cancelled) return;
            setLoading(false);
        });

        // Detect installed agents (local or remote)
        if (isRemote && remoteServerId) {
            setDetecting(true);
            api.bridgeDetectAgentsRemote(remoteServerId).then(remoteStatuses => {
                if (cancelled) return;
                const mapped: AgentStatus[] = remoteStatuses.map(r => ({ id: r.id, name: r.name, installed: r.installed, running: r.running }));
                setAgentStatuses(mapped);
                // If no enabled agent is installed, clear stale role selection
                const anyAvailable = AGENT_TOOLS.some(a => a.enabled && mapped.find(s => s.id === a.id)?.installed);
                if (!anyAvailable) {
                    setLocalSelected(null);
                    onSelectRole('', '', '');
                    onSelectAgent('');
                }
                setDetecting(false);
            }).catch(() => {
                if (cancelled) return;
                // Bridge not reachable — mark ALL agents as not installed + clear role
                const allNotInstalled: AgentStatus[] = AGENT_TOOLS.map(a => ({ id: a.id, name: a.name, installed: false }));
                setAgentStatuses(allNotInstalled);
                setLocalSelected(null);
                onSelectRole('', '', '');
                onSelectAgent('');
                setConnectionError(true);
                setDetecting(false);
            });
        } else if (!isRemote) {
            setDetecting(true);
            api.detectLocalAgents().then(statuses => {
                if (cancelled) return;
                setAgentStatuses(statuses);
                // If no enabled agent is installed locally, clear stale role selection
                const anyAvailable = AGENT_TOOLS.some(a => a.enabled && statuses.find(s => s.id === a.id)?.installed);
                if (!anyAvailable) {
                    setLocalSelected(null);
                    onSelectRole('', '', '');
                    onSelectAgent('');
                }
                setDetecting(false);
            }).catch(() => {
                if (cancelled) return;
                // Local detection error — mark all as not installed + clear selections
                const allNotInstalled: AgentStatus[] = AGENT_TOOLS.map(a => ({ id: a.id, name: a.name, installed: false }));
                setAgentStatuses(allNotInstalled);
                setLocalSelected(null);
                onSelectRole('', '', '');
                onSelectAgent('');
                setConnectionError(true);
                setDetecting(false);
            });
        }

        return () => { cancelled = true; };
    }, [isOpen, locale]);

    if (!isOpen) return null;

    // Check if current agent is available
    const currentAgentTool = AGENT_TOOLS.find(a => a.name === selectedAgent);
    const currentAgentStatus = currentAgentTool ? agentStatuses.find(s => s.id === currentAgentTool.id) : null;
    const currentAgentAvailable = currentAgentTool?.enabled && (detecting ? false : (currentAgentStatus?.installed ?? false));

    const handleSelect = (role: RoleEntry) => {
        if (!currentAgentAvailable) return; // Block role selection when agent not installed
        setLocalSelected(role.id);
        onSelectRole(role.id, role.name, role.filePath);
    };

    const handleClear = () => {
        setLocalSelected(null);
        onSelectRole('', '', '');
    };

    const filteredRoles = activeCat === 'all' ? roles : roles.filter(r => r.category === activeCat);

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />

            <div className="relative w-[100vw] h-[100vh] flex flex-col bg-cyber-bg overflow-hidden">
                {/* Header — back button + page-style title */}
                <div className="flex items-center gap-3 px-5 py-3 flex-shrink-0">
                    <button onClick={onClose} className="w-9 h-9 flex items-center justify-center rounded-lg text-cyber-accent hover:bg-cyber-accent/15 transition-colors">
                        <ArrowLeft size={18} />
                    </button>
                    <h2 className="text-2xl cjk-title leading-8 flex-1 flex items-center gap-2.5">
                        {t('channel.selectRoleAgent')}
                        {detecting && (
                            <Loader2 size={18} className="text-cyber-accent/70 animate-spin" />
                        )}
                        {connectionError && !detecting && (
                            <span className="text-xs font-mono font-normal text-red-400">{t('error.serverUnreachable')}</span>
                        )}
                    </h2>
                    <button onClick={onClose} className="w-9 h-9 flex items-center justify-center rounded-lg text-cyber-text-muted/40 hover:text-cyber-text hover:bg-white/10 transition-colors">
                        <X size={18} />
                    </button>
                </div>

                {/* Agent tool selector */}
                <div className="flex items-center gap-3 px-5 pt-2 pb-3 flex-shrink-0">
                    {AGENT_TOOLS.map(agent => {
                        const isActive = selectedAgent === agent.name;
                        const status = agentStatuses.find(s => s.id === agent.id);
                        const isAvailable = agent.enabled && (detecting ? false : (status?.installed ?? false));
                        return (
                            <div
                                key={agent.name}
                                onClick={() => { if (isAvailable) onSelectAgent(agent.name); }}
                                title={!agent.enabled ? 'Coming Soon' : ''}
                                className={`flex items-center gap-2 px-4 py-2 rounded-card text-sm font-mono transition-all ${
                                    !isAvailable
                                        ? 'border border-cyber-border/30 bg-black/60 text-cyber-text-muted/30 cursor-not-allowed opacity-40'
                                        : isActive
                                            ? 'border border-cyber-accent bg-cyber-accent/10 shadow-cyber-card text-cyber-accent cursor-pointer'
                                            : 'border border-cyber-border shadow-cyber-card bg-black/80 text-cyber-text-muted/70 hover:border-cyber-accent/30 hover:bg-black/90 cursor-pointer'
                                }`}
                            >
                                <img src={agent.icon} alt={agent.name} className={`w-5 h-5 ${isActive && isAvailable ? '' : 'opacity-50 grayscale'}`} />
                                <span>{agent.name}</span>
                                {status?.running && <span className="w-2 h-2 rounded-full bg-cyber-accent animate-pulse" />}
                            </div>
                        );
                    })}
                </div>

                {/* Category tabs — dynamically loaded, auto-wrap */}
                <div className="flex flex-wrap items-center gap-1.5 px-5 py-2.5 flex-shrink-0">
                    <button
                        onClick={() => setActiveCat('all')}
                        className={`px-3 py-1 rounded-md text-xs font-mono whitespace-nowrap transition-all ${
                            activeCat === 'all'
                                ? 'bg-cyber-accent/15 text-cyber-accent border border-cyber-accent/40'
                                : 'text-cyber-text-muted/60 border border-transparent hover:text-cyber-accent/80 hover:bg-cyber-accent/5'
                        }`}
                    >
                        {allLabel}
                    </button>
                    {categories.map(cat => (
                        <button
                            key={cat.id}
                            onClick={() => setActiveCat(cat.id)}
                            className={`px-3 py-1 rounded-md text-xs font-mono whitespace-nowrap transition-all ${
                                activeCat === cat.id
                                    ? 'bg-cyber-accent/15 text-cyber-accent border border-cyber-accent/40'
                                    : 'text-cyber-text-muted/60 border border-transparent hover:text-cyber-accent/80 hover:bg-cyber-accent/5'
                            }`}
                        >
                            {cat.label}
                        </button>
                    ))}
                </div>

                {/* Card grid */}
                <div className="flex-1 overflow-y-auto slim-scroll custom-scrollbar p-4">
                    {loading ? (
                        <div className="flex items-center justify-center h-full">
                            <Loader2 size={24} className="text-cyber-accent animate-spin" />
                        </div>
                    ) : (
                        <div className="grid grid-cols-6 gap-5">
                            {/* No-role (clear) card */}
                            <div
                                onClick={() => { if (currentAgentAvailable) handleClear(); }}
                                className={`relative border rounded-card cursor-pointer transition-all overflow-hidden group ${
                                    localSelected === null
                                        ? 'border-cyber-accent shadow-[0_0_12px_rgba(0,255,157,0.3)]'
                                        : 'border-cyber-border bg-black/80 shadow-cyber-card hover:border-cyber-accent/40 hover:shadow-[0_0_8px_rgba(0,255,157,0.1)]'
                                }`}
                            >
                                <div className="aspect-[5/8] overflow-hidden bg-black/60 flex items-center justify-center">
                                    <img src="/none.png" alt="" className="w-full h-full object-cover object-center" />
                                </div>
                                {localSelected === null && (
                                    <div className="absolute top-2 right-2 w-7 h-7 rounded-lg flex items-center justify-center z-10 bg-cyber-accent ring-2 ring-black/40"
                                         style={{ boxShadow: '0 0 10px rgba(0,255,157,0.6), 0 2px 8px rgba(0,0,0,0.5)' }}>
                                        <Check size={14} className="text-black" strokeWidth={3} />
                                    </div>
                                )}
                            </div>
                            {filteredRoles.map(role => {
                                const isSelected = localSelected === role.id;
                                return (
                                    <div
                                        key={role.id}
                                        onClick={() => handleSelect(role)}
                                        className={`relative border rounded-card transition-all overflow-hidden group ${
                                            !currentAgentAvailable
                                                ? 'border-cyber-border/30 bg-black/60 opacity-40 cursor-not-allowed'
                                                : isSelected
                                                    ? 'border-cyber-accent shadow-[0_0_12px_rgba(0,255,157,0.3)] cursor-pointer'
                                                    : 'border-cyber-border bg-black/80 shadow-cyber-card hover:border-cyber-accent/40 hover:shadow-[0_0_8px_rgba(0,255,157,0.1)] cursor-pointer'
                                        }`}
                                    >
                                        {/* Image with skeleton loading */}
                                        <div className="aspect-[5/8] overflow-hidden bg-black/60 relative">
                                            <div className="absolute inset-0 bg-gradient-to-br from-cyber-border/20 to-black/40 animate-pulse" />
                                            <img
                                                src={role.img || role.fallbackImg}
                                                alt={role.name}
                                                className="w-full h-full object-cover object-center relative"
                                                loading="lazy"
                                                onLoad={(e) => { (e.target as HTMLElement).style.opacity = '1'; }}
                                                onError={(e) => {
                                                    const el = e.target as HTMLImageElement;
                                                    if (role.fallbackImg && el.src !== role.fallbackImg) {
                                                        el.src = role.fallbackImg;
                                                    } else if (!el.src.endsWith('/none.png')) {
                                                        el.src = '/none.png';
                                                    }
                                                }}
                                                style={{ opacity: 0, transition: 'opacity 0.3s ease-in' }}
                                            />
                                        </div>

                                        {/* ✓ badge */}
                                        {isSelected && (
                                            <div className="absolute top-2 right-2 w-7 h-7 rounded-lg flex items-center justify-center z-10 bg-cyber-accent ring-2 ring-black/40"
                                                 style={{ boxShadow: '0 0 10px rgba(0,255,157,0.6), 0 2px 8px rgba(0,0,0,0.5)' }}>
                                                <Check size={14} className="text-black" strokeWidth={3} />
                                            </div>
                                        )}

                                        {/* Gradient overlay */}
                                        <div className="absolute inset-x-0 bottom-0 pointer-events-none"
                                             style={{
                                                 height: '60%',
                                                 background: 'linear-gradient(to top, rgba(0,0,0,0.88) 0%, rgba(0,0,0,0.5) 40%, transparent 100%)',
                                             }}
                                        />

                                        {/* Text overlay */}
                                        <div className="absolute inset-x-0 bottom-0 px-3 pb-3 flex flex-col items-center"
                                             style={{ backfaceVisibility: 'hidden' }}>
                                            <div
                                                className="text-sm font-bold text-center leading-tight line-clamp-2"
                                                style={{
                                                    color: '#00ff9d',
                                                    textShadow: '0 0 8px rgba(0,0,0,0.9), 0 0 16px rgba(0,0,0,0.7), 0 1px 3px rgba(0,0,0,1)',
                                                    backfaceVisibility: 'hidden',
                                                    wordBreak: 'keep-all',
                                                }}
                                            >
                                                {role.name}
                                            </div>

                                            <div className="flex flex-col items-center overflow-hidden transition-all duration-300 ease-out max-h-0 group-hover:max-h-[60px] opacity-0 group-hover:opacity-100"
                                                 style={{ backfaceVisibility: 'hidden' }}>
                                                <div className="w-16 h-px mt-1.5 mb-1" style={{
                                                    background: 'linear-gradient(90deg, transparent, #00ff9d, transparent)',
                                                }} />
                                                <div
                                                    className="text-[13px] text-center leading-snug line-clamp-2"
                                                    style={{
                                                        color: 'rgba(200,200,200,0.8)',
                                                        textShadow: '0 0 6px rgba(0,0,0,0.9), 0 0 12px rgba(0,0,0,0.8), 0 1px 2px rgba(0,0,0,1)',
                                                        backfaceVisibility: 'hidden',
                                                    }}
                                                >
                                                    {role.description}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
};
