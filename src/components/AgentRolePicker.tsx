// AgentRolePicker — Simple role card selector modal
// Vertical image cards with hover slide-up text effect + category filter
import React, { useState } from 'react';
import { X, Check } from 'lucide-react';

// ── Categories ──
const CATEGORIES = [
    { id: 'all', label: 'All' },
    { id: 'engineering', label: 'Engineering' },
    { id: 'design', label: 'Design' },
    { id: 'marketing', label: 'Marketing' },
    { id: 'data', label: 'Data & AI' },
    { id: 'product', label: 'Product' },
    { id: 'testing', label: 'Testing' },
];

// ── 18 hardcoded roles with images from public/role/ ──
const ROLES = [
    { id: 'frontend-dev', name: 'Frontend Developer', desc: 'Modern web UI specialist', img: '/role/1119642287_IGDB-285x380.jpg', cat: 'engineering' },
    { id: 'backend-architect', name: 'Backend Architect', desc: 'Server-side systems expert', img: '/role/116747788-285x380.png', cat: 'engineering' },
    { id: 'ai-engineer', name: 'AI Engineer', desc: 'Machine learning implementation', img: '/role/1329153872_IGDB-285x380.jpg', cat: 'data' },
    { id: 'game-designer', name: 'Game Designer', desc: 'Game mechanics and systems', img: '/role/1435206302_IGDB-285x380.jpg', cat: 'design' },
    { id: 'security-expert', name: 'Security Expert', desc: 'Security and vulnerability analysis', img: '/role/1597660489_IGDB-285x380.jpg', cat: 'engineering' },
    { id: 'data-scientist', name: 'Data Scientist', desc: 'Data analysis and insights', img: '/role/1630982727_IGDB-285x380.jpg', cat: 'data' },
    { id: 'devops-master', name: 'DevOps Master', desc: 'CI/CD and infrastructure', img: '/role/21779-285x380.jpg', cat: 'engineering' },
    { id: 'ui-designer', name: 'UI Designer', desc: 'Visual interface design', img: '/role/29307_IGDB-285x380.jpg', cat: 'design' },
    { id: 'code-reviewer', name: 'Code Reviewer', desc: 'Code quality and review', img: '/role/509538_IGDB-285x380.jpg', cat: 'engineering' },
    { id: 'product-manager', name: 'Product Manager', desc: 'Product strategy and planning', img: '/role/509658-285x380.jpg', cat: 'product' },
    { id: 'creative-writer', name: 'Creative Writer', desc: 'Content and story creation', img: '/role/511224-285x380.jpg', cat: 'marketing' },
    { id: 'translator', name: 'Translator', desc: 'Multi-language translation', img: '/role/512864_IGDB-285x380.jpg', cat: 'marketing' },
    { id: 'seo-specialist', name: 'SEO Specialist', desc: 'Search optimization expert', img: '/role/513181_IGDB-285x380.jpg', cat: 'marketing' },
    { id: 'prompt-engineer', name: 'Prompt Engineer', desc: 'AI prompt optimization', img: '/role/515025-285x380.png', cat: 'data' },
    { id: 'api-tester', name: 'API Tester', desc: 'API testing and validation', img: '/role/516575-285x380.png', cat: 'testing' },
    { id: 'sales-strategist', name: 'Sales Strategist', desc: 'Sales pipeline and deals', img: '/role/55453844_IGDB-285x380.jpg', cat: 'product' },
    { id: 'support-hero', name: 'Support Hero', desc: 'Customer support specialist', img: '/role/66082-285x380.jpg', cat: 'product' },
    { id: 'research-analyst', name: 'Research Analyst', desc: 'Deep research and analysis', img: '/role/66366_IGDB-285x380.jpg', cat: 'data' },
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
    const [activeCat, setActiveCat] = useState('all');

    if (!isOpen) return null;

    const handleSelect = (roleId: string, roleName: string) => {
        setLocalSelected(roleId);
        onSelectRole(roleId, roleName);
    };

    const filteredRoles = activeCat === 'all' ? ROLES : ROLES.filter(r => r.cat === activeCat);

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />

            <div className="relative w-[90vw] h-[85vh] flex flex-col bg-cyber-bg border border-cyber-border rounded-card shadow-cyber-card overflow-hidden">
                {/* Header */}
                <div className="flex items-center justify-between px-5 py-3 border-b border-cyber-border/50 flex-shrink-0">
                    <span className="text-cyber-accent text-sm font-mono font-bold">
                        Select Role · {agentName}
                    </span>
                    <button onClick={onClose} className="text-cyber-text-muted/50 hover:text-cyber-accent transition-colors">
                        <X size={16} />
                    </button>
                </div>

                {/* Category tabs */}
                <div className="flex items-center gap-1.5 px-5 py-2.5 border-b border-cyber-border/30 flex-shrink-0 overflow-x-auto">
                    {CATEGORIES.map(cat => (
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
                    <div className="grid grid-cols-6 gap-3">
                        {filteredRoles.map(role => {
                            const isSelected = localSelected === role.id;
                            return (
                                <div
                                    key={role.id}
                                    onClick={() => handleSelect(role.id, role.name)}
                                    className={`relative border rounded-card cursor-pointer transition-all overflow-hidden group ${
                                        isSelected
                                            ? 'border-cyber-accent shadow-[0_0_12px_rgba(0,255,157,0.3)]'
                                            : 'border-cyber-border bg-black/80 shadow-cyber-card hover:border-cyber-accent/40 hover:shadow-[0_0_8px_rgba(0,255,157,0.1)]'
                                    }`}
                                >
                                    {/* Image */}
                                    <div className="aspect-[5/8] overflow-hidden bg-black/60">
                                        <img
                                            src={role.img}
                                            alt={role.name}
                                            className="w-full h-full object-cover object-center"
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
                                            className="text-base font-bold text-center leading-tight line-clamp-2"
                                            style={{
                                                color: '#00ff9d',
                                                textShadow: '0 0 8px rgba(0,0,0,0.9), 0 0 16px rgba(0,0,0,0.7), 0 1px 3px rgba(0,0,0,1)',
                                                backfaceVisibility: 'hidden',
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
