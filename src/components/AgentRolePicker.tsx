// AgentRolePicker — Simple role card selector modal
// Vertical image cards with hover slide-up text effect
import React, { useState, useCallback } from 'react';
import { X, RefreshCw, Check, UserPlus } from 'lucide-react';

// ── 18 hardcoded roles with images from public/role/ ──
const ROLES = [
    { id: 'frontend-dev', name: 'Frontend Developer', desc: 'Modern web UI specialist', img: '/role/1119642287_IGDB-285x380.jpg' },
    { id: 'backend-architect', name: 'Backend Architect', desc: 'Server-side systems expert', img: '/role/116747788-285x380.png' },
    { id: 'ai-engineer', name: 'AI Engineer', desc: 'Machine learning implementation', img: '/role/1329153872_IGDB-285x380.jpg' },
    { id: 'game-designer', name: 'Game Designer', desc: 'Game mechanics and systems', img: '/role/1435206302_IGDB-285x380.jpg' },
    { id: 'security-expert', name: 'Security Expert', desc: 'Security and vulnerability analysis', img: '/role/1597660489_IGDB-285x380.jpg' },
    { id: 'data-scientist', name: 'Data Scientist', desc: 'Data analysis and insights', img: '/role/1630982727_IGDB-285x380.jpg' },
    { id: 'devops-master', name: 'DevOps Master', desc: 'CI/CD and infrastructure', img: '/role/21779-285x380.jpg' },
    { id: 'ui-designer', name: 'UI Designer', desc: 'Visual interface design', img: '/role/29307_IGDB-285x380.jpg' },
    { id: 'code-reviewer', name: 'Code Reviewer', desc: 'Code quality and review', img: '/role/509538_IGDB-285x380.jpg' },
    { id: 'product-manager', name: 'Product Manager', desc: 'Product strategy and planning', img: '/role/509658-285x380.jpg' },
    { id: 'creative-writer', name: 'Creative Writer', desc: 'Content and story creation', img: '/role/511224-285x380.jpg' },
    { id: 'translator', name: 'Translator', desc: 'Multi-language translation', img: '/role/512864_IGDB-285x380.jpg' },
    { id: 'seo-specialist', name: 'SEO Specialist', desc: 'Search optimization expert', img: '/role/513181_IGDB-285x380.jpg' },
    { id: 'prompt-engineer', name: 'Prompt Engineer', desc: 'AI prompt optimization', img: '/role/515025-285x380.png' },
    { id: 'api-tester', name: 'API Tester', desc: 'API testing and validation', img: '/role/516575-285x380.png' },
    { id: 'sales-strategist', name: 'Sales Strategist', desc: 'Sales pipeline and deals', img: '/role/55453844_IGDB-285x380.jpg' },
    { id: 'support-hero', name: 'Support Hero', desc: 'Customer support specialist', img: '/role/66082-285x380.jpg' },
    { id: 'research-analyst', name: 'Research Analyst', desc: 'Deep research and analysis', img: '/role/66366_IGDB-285x380.jpg' },
];

// ── Component ──

interface AgentRolePickerProps {
    isOpen: boolean;
    onClose: () => void;
    selectedRole: string | null;
    onSelectRole: (roleId: string, roleName: string) => void;
    agentName: string;
}

export const AgentRolePicker: React.FC<AgentRolePickerProps> = ({
    isOpen,
    onClose,
    selectedRole,
    onSelectRole,
    agentName,
}) => {
    const [localSelected, setLocalSelected] = useState<string | null>(selectedRole);
    const [connecting, setConnecting] = useState<string | null>(null);
    const [connected, setConnected] = useState<string | null>(selectedRole);

    const handlePlugClick = useCallback((e: React.MouseEvent, roleId: string, roleName: string) => {
        e.stopPropagation();
        if (connecting) return;
        setConnecting(roleId);
        setTimeout(() => {
            setConnecting(null);
            setConnected(roleId);
            onSelectRole(roleId, roleName);
        }, 1500);
    }, [connecting, onSelectRole]);

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />

            <div className="relative w-[90vw] max-h-[85vh] flex flex-col bg-cyber-bg border border-cyber-border rounded-card shadow-cyber-card overflow-hidden">
                {/* Header */}
                <div className="flex items-center justify-between px-5 py-3 border-b border-cyber-border/50 flex-shrink-0">
                    <span className="text-cyber-accent text-sm font-mono font-bold">
                        Select Role · {agentName}
                    </span>
                    <button onClick={onClose} className="text-cyber-text-muted/50 hover:text-cyber-accent transition-colors">
                        <X size={16} />
                    </button>
                </div>

                {/* Card grid */}
                <div className="flex-1 overflow-y-auto slim-scroll custom-scrollbar p-4">
                    <div className="grid grid-cols-6 gap-3">
                        {ROLES.map(role => {
                            const isSelected = localSelected === role.id;
                            const isConnecting = connecting === role.id;
                            const isConnected = connected === role.id;
                            return (
                                <div
                                    key={role.id}
                                    onClick={() => setLocalSelected(role.id)}
                                    className={`relative border rounded-card cursor-pointer transition-all overflow-hidden group ${
                                        isSelected
                                            ? 'border-cyber-accent shadow-[0_0_12px_rgba(0,255,157,0.3)]'
                                            : 'border-cyber-border bg-black/80 shadow-cyber-card hover:border-cyber-accent/40 hover:shadow-[0_0_8px_rgba(0,255,157,0.1)]'
                                    }`}
                                >
                                    {/* Image — static, no zoom */}
                                    <div className="aspect-video overflow-hidden bg-black/60">
                                        <img
                                            src={role.img}
                                            alt={role.name}
                                            className="w-full h-full object-cover object-center"
                                        />
                                    </div>

                                    {/* UserPlus button — top right */}
                                    {isSelected && (
                                        <button
                                            onClick={(e) => handlePlugClick(e, role.id, role.name)}
                                            className={`absolute top-2 right-2 w-7 h-7 rounded-lg flex items-center justify-center transition-all z-10 ${
                                                isConnected
                                                    ? 'bg-cyber-accent shadow-[0_0_8px_rgba(0,255,157,0.5)]'
                                                    : 'bg-cyber-accent hover:brightness-110 shadow-[0_0_6px_rgba(0,255,157,0.3)]'
                                            }`}
                                        >
                                            {isConnecting ? (
                                                <RefreshCw size={14} className="text-black animate-spin" />
                                            ) : isConnected ? (
                                                <Check size={14} className="text-black" strokeWidth={3} />
                                            ) : (
                                                <UserPlus size={14} className="text-black" strokeWidth={2.5} />
                                            )}
                                        </button>
                                    )}

                                    {/* Gradient overlay — always visible for readability */}
                                    <div className="absolute inset-x-0 bottom-0 pointer-events-none"
                                         style={{
                                             height: '60%',
                                             background: 'linear-gradient(to top, rgba(0,0,0,0.88) 0%, rgba(0,0,0,0.5) 40%, transparent 100%)',
                                         }}
                                    />

                                    {/* Text overlay — slides up on hover */}
                                    <div className="absolute inset-x-0 bottom-0 px-3 flex flex-col items-center transition-transform duration-300 ease-out translate-y-[28px] group-hover:translate-y-0"
                                         style={{ paddingBottom: '12px', willChange: 'transform', backfaceVisibility: 'hidden' }}>
                                        {/* Name — always partially visible */}
                                        <div
                                            className="text-sm font-bold text-center leading-tight line-clamp-2"
                                            style={{
                                                color: isSelected ? '#00ff9d' : '#ffffff',
                                                textShadow: '0 0 8px rgba(0,0,0,0.9), 0 0 16px rgba(0,0,0,0.7), 0 1px 3px rgba(0,0,0,1)',
                                                backfaceVisibility: 'hidden',
                                            }}
                                        >
                                            {role.name}
                                        </div>

                                        {/* Divider + Description — revealed together via translate */}
                                        <div className="flex flex-col items-center opacity-0 group-hover:opacity-100 transition-opacity duration-200 delay-100"
                                             style={{ backfaceVisibility: 'hidden' }}>
                                            <div className="w-16 h-px my-1.5" style={{
                                                background: isSelected
                                                    ? 'linear-gradient(90deg, transparent, #00ff9d, transparent)'
                                                    : 'linear-gradient(90deg, transparent, rgba(255,255,255,0.4), transparent)',
                                            }} />
                                            <div
                                                className="text-[11px] text-center leading-snug line-clamp-2"
                                                style={{
                                                    color: isSelected ? 'rgba(0,255,157,0.7)' : 'rgba(255,255,255,0.6)',
                                                    textShadow: '0 0 6px rgba(0,0,0,0.9), 0 0 12px rgba(0,0,0,0.8), 0 1px 2px rgba(0,0,0,1)',
                                                    backfaceVisibility: 'hidden',
                                                }}
                                            >
                                                {role.desc}
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            );
                        })}
                    </div>
                </div>
            </div>
        </div>
    );
};
