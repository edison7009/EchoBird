// AgentRolePicker — Simple role card selector modal
// Vertical image cards like the reference screenshot
import React from 'react';
import { X } from 'lucide-react';

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
    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            {/* Backdrop */}
            <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />

            {/* Modal */}
            <div className="relative w-[80vw] max-h-[80vh] flex flex-col bg-cyber-bg border border-cyber-border rounded-card shadow-cyber-card overflow-hidden">

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
                            const isSelected = selectedRole === role.id;
                            return (
                                <div
                                    key={role.id}
                                    onClick={() => { onSelectRole(role.id, role.name); onClose(); }}
                                    className={`border rounded-card cursor-pointer transition-all overflow-hidden ${
                                        isSelected
                                            ? 'border-cyber-accent shadow-[0_0_12px_rgba(0,255,157,0.3)]'
                                            : 'border-cyber-border bg-black/80 shadow-cyber-card hover:border-cyber-accent/40 hover:shadow-[0_0_8px_rgba(0,255,157,0.1)]'
                                    }`}
                                >
                                    {/* Image — center crop */}
                                    <div className="aspect-[3/4] overflow-hidden bg-black/60">
                                        <img
                                            src={role.img}
                                            alt={role.name}
                                            className="w-full h-full object-cover object-center"
                                        />
                                    </div>
                                    {/* Info */}
                                    <div className="p-2.5">
                                        <div className={`text-xs font-mono font-bold truncate ${isSelected ? 'text-cyber-accent' : 'text-cyber-accent/80'}`}>
                                            {role.name}
                                        </div>
                                        <div className="text-[10px] font-mono text-cyber-text-muted/40 mt-0.5 truncate">
                                            {role.desc}
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
