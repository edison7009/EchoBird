// AgentRolePicker — Modal for selecting agent role/identity
// Opens when user clicks the switch icon on agent tabs in Channels
import React, { useState } from 'react';
import { X, Search } from 'lucide-react';

// ── Role data (hardcoded from agency-agents) ──
interface AgentRole {
    id: string;
    name: string;
    description: string;
    division: string;
}

interface Division {
    id: string;
    icon: string;
    name: string;
}

const DIVISIONS: Division[] = [
    { id: 'all', icon: '🌐', name: 'All' },
    { id: 'engineering', icon: '💻', name: 'Engineering' },
    { id: 'design', icon: '🎨', name: 'Design' },
    { id: 'marketing', icon: '📢', name: 'Marketing' },
    { id: 'product', icon: '📊', name: 'Product' },
    { id: 'game-dev', icon: '🎮', name: 'Game Dev' },
    { id: 'testing', icon: '🧪', name: 'Testing' },
    { id: 'support', icon: '🛟', name: 'Support' },
    { id: 'sales', icon: '💼', name: 'Sales' },
    { id: 'project-mgmt', icon: '🎬', name: 'Project Mgmt' },
    { id: 'specialized', icon: '🎯', name: 'Specialized' },
];

const ROLES: AgentRole[] = [
    // Engineering
    { id: 'frontend-developer', name: 'Frontend Developer', description: 'Modern web application and UI implementation specialist', division: 'engineering' },
    { id: 'backend-architect', name: 'Backend Architect', description: 'Server-side systems design and scalability expert', division: 'engineering' },
    { id: 'devops-automator', name: 'DevOps Automator', description: 'CI/CD pipeline and infrastructure automation', division: 'engineering' },
    { id: 'security-engineer', name: 'Security Engineer', description: 'Application security and vulnerability analysis', division: 'engineering' },
    { id: 'ai-engineer', name: 'AI Engineer', description: 'Machine learning and AI systems implementation', division: 'engineering' },
    { id: 'mobile-app-builder', name: 'Mobile App Builder', description: 'Cross-platform mobile application development', division: 'engineering' },
    { id: 'code-reviewer', name: 'Code Reviewer', description: 'Code quality analysis and review specialist', division: 'engineering' },
    { id: 'database-optimizer', name: 'Database Optimizer', description: 'Database performance tuning and query optimization', division: 'engineering' },
    { id: 'software-architect', name: 'Software Architect', description: 'System design and architecture decision making', division: 'engineering' },
    { id: 'technical-writer', name: 'Technical Writer', description: 'Technical documentation and API reference creation', division: 'engineering' },
    // Design
    { id: 'ui-designer', name: 'UI Designer', description: 'Visual interface design and design systems', division: 'design' },
    { id: 'ux-researcher', name: 'UX Researcher', description: 'User research and usability testing', division: 'design' },
    { id: 'brand-guardian', name: 'Brand Guardian', description: 'Brand identity consistency and guidelines', division: 'design' },
    { id: 'visual-storyteller', name: 'Visual Storyteller', description: 'Data visualization and infographic design', division: 'design' },
    // Marketing
    { id: 'content-strategist', name: 'Content Strategist', description: 'Content planning and editorial strategy', division: 'marketing' },
    { id: 'seo-specialist', name: 'SEO Specialist', description: 'Search engine optimization and analytics', division: 'marketing' },
    { id: 'social-media-manager', name: 'Social Media Manager', description: 'Social media strategy and community building', division: 'marketing' },
    // Product
    { id: 'product-manager', name: 'Product Manager', description: 'Product strategy and feature prioritization', division: 'product' },
    { id: 'data-analyst', name: 'Data Analyst', description: 'Data analysis and business intelligence', division: 'product' },
    // Game Dev
    { id: 'game-designer', name: 'Game Designer', description: 'Game mechanics and systems design', division: 'game-dev' },
    { id: 'level-designer', name: 'Level Designer', description: 'Level layout and gameplay flow design', division: 'game-dev' },
    { id: 'technical-artist', name: 'Technical Artist', description: 'Shader programming and visual effects', division: 'game-dev' },
    { id: 'narrative-designer', name: 'Narrative Designer', description: 'Story writing and dialogue systems', division: 'game-dev' },
    { id: 'game-audio-engineer', name: 'Game Audio Engineer', description: 'Game sound design and audio implementation', division: 'game-dev' },
    // Testing
    { id: 'api-tester', name: 'API Tester', description: 'API testing and validation specialist', division: 'testing' },
    { id: 'performance-benchmarker', name: 'Performance Benchmarker', description: 'Performance testing and benchmarking', division: 'testing' },
    { id: 'accessibility-auditor', name: 'Accessibility Auditor', description: 'Accessibility compliance and testing', division: 'testing' },
    // Support
    { id: 'support-responder', name: 'Support Responder', description: 'Customer support and issue resolution', division: 'support' },
    { id: 'analytics-reporter', name: 'Analytics Reporter', description: 'Analytics reporting and dashboard creation', division: 'support' },
    // Sales
    { id: 'outbound-strategist', name: 'Outbound Strategist', description: 'Outbound sales strategy and prospecting', division: 'sales' },
    { id: 'sales-engineer', name: 'Sales Engineer', description: 'Technical sales demonstrations and POCs', division: 'sales' },
    // Project Management
    { id: 'project-shepherd', name: 'Project Shepherd', description: 'Project coordination and stakeholder management', division: 'project-mgmt' },
    { id: 'studio-producer', name: 'Studio Producer', description: 'Production planning and resource management', division: 'project-mgmt' },
    // Specialized
    { id: 'prompt-engineer', name: 'Prompt Engineer', description: 'AI prompt optimization and engineering', division: 'specialized' },
    { id: 'research-analyst', name: 'Research Analyst', description: 'Deep research and competitive analysis', division: 'specialized' },
    { id: 'translator', name: 'Translator', description: 'Multi-language translation and localization', division: 'specialized' },
];

// ── Component ──

interface AgentRolePickerProps {
    isOpen: boolean;
    onClose: () => void;
    selectedRole: string | null;
    onSelectRole: (roleId: string, roleName: string) => void;
    agentName: string; // which agent tool (OpenClaw, Claude Code, etc.)
}

export const AgentRolePicker: React.FC<AgentRolePickerProps> = ({
    isOpen,
    onClose,
    selectedRole,
    onSelectRole,
    agentName,
}) => {
    const [activeDivision, setActiveDivision] = useState('all');
    const [search, setSearch] = useState('');

    if (!isOpen) return null;

    const filtered = ROLES.filter(r => {
        if (activeDivision !== 'all' && r.division !== activeDivision) return false;
        if (search) {
            const q = search.toLowerCase();
            return r.name.toLowerCase().includes(q) || r.description.toLowerCase().includes(q);
        }
        return true;
    });

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            {/* Backdrop */}
            <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />

            {/* Modal */}
            <div className="relative w-[720px] max-w-[90vw] max-h-[80vh] flex flex-col bg-cyber-bg border border-cyber-border rounded-card shadow-cyber-card overflow-hidden">

                {/* Header */}
                <div className="flex items-center justify-between px-5 py-3 border-b border-cyber-border/50 flex-shrink-0">
                    <div className="flex items-center gap-2 font-mono">
                        <span className="text-cyber-accent text-sm font-bold">Select Role</span>
                        <span className="text-cyber-text-muted/50 text-xs">· {agentName}</span>
                    </div>
                    <button onClick={onClose} className="text-cyber-text-muted/50 hover:text-cyber-accent transition-colors">
                        <X size={16} />
                    </button>
                </div>

                {/* Search */}
                <div className="px-5 py-2 border-b border-cyber-border/30 flex-shrink-0">
                    <div className="flex items-center gap-2 px-3 py-1.5 rounded-card bg-black/60 border border-cyber-border/40">
                        <Search size={14} className="text-cyber-text-muted/40" />
                        <input
                            type="text"
                            value={search}
                            onChange={e => setSearch(e.target.value)}
                            placeholder="Search roles..."
                            className="flex-1 bg-transparent text-xs text-cyber-text-primary font-mono outline-none placeholder:text-cyber-text-muted/30"
                        />
                    </div>
                </div>

                {/* Division tabs */}
                <div className="px-5 py-2 flex gap-1.5 flex-wrap border-b border-cyber-border/30 flex-shrink-0">
                    {DIVISIONS.map(div => (
                        <button
                            key={div.id}
                            onClick={() => setActiveDivision(div.id)}
                            className={`px-2.5 py-1 rounded-card text-xs font-mono transition-all ${
                                activeDivision === div.id
                                    ? 'border border-cyber-accent bg-cyber-accent/10 text-cyber-accent'
                                    : 'border border-cyber-border/40 text-cyber-text-muted/50 hover:border-cyber-accent/30 hover:text-cyber-text-muted/70'
                            }`}
                        >
                            {div.icon} {div.name}
                        </button>
                    ))}
                </div>

                {/* Role cards grid */}
                <div className="flex-1 overflow-y-auto slim-scroll custom-scrollbar p-4">
                    <div className="grid grid-cols-3 gap-3">
                        {filtered.map(role => {
                            const isSelected = selectedRole === role.id;
                            const div = DIVISIONS.find(d => d.id === role.division);
                            return (
                                <div
                                    key={role.id}
                                    onClick={() => { onSelectRole(role.id, role.name); onClose(); }}
                                    className={`p-4 border rounded-card cursor-pointer transition-all flex flex-col gap-2 ${
                                        isSelected
                                            ? 'border-cyber-accent bg-cyber-accent/10 shadow-[0_0_10px_rgba(0,255,157,0.2)]'
                                            : 'border-cyber-border bg-black/80 shadow-cyber-card hover:border-cyber-accent/40 hover:bg-black/90'
                                    }`}
                                >
                                    {/* Division tag */}
                                    <span className="text-[10px] font-mono text-cyber-text-muted/40 uppercase tracking-wider">
                                        {div?.icon} {div?.name}
                                    </span>
                                    {/* Role name */}
                                    <span className={`text-sm font-mono font-bold ${isSelected ? 'text-cyber-accent' : 'text-cyber-accent/80'}`}>
                                        {role.name}
                                    </span>
                                    {/* Description */}
                                    <span className="text-xs font-mono text-cyber-text-muted/50 leading-relaxed line-clamp-2">
                                        {role.description}
                                    </span>
                                </div>
                            );
                        })}
                    </div>
                    {filtered.length === 0 && (
                        <div className="text-center py-8 text-cyber-text-muted/30 text-xs font-mono">
                            No roles found
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
};
