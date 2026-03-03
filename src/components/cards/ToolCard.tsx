// ToolCard component
import React from 'react';
import { useI18n } from '../../hooks/useI18n';

export interface ToolCardProps {
    id: string;
    name: string;
    version?: string;
    installed: boolean;
    path?: string;
    detectedPath?: string;
    configPath?: string;
    skillsCount?: number;
    installedSkillsCount?: number; // from backend DetectedTool
    activeModel?: string;
    website?: string;
    iconBase64?: string;
    names?: Record<string, string>;  // i18n names
    selected?: boolean;
    onClick?: () => void;
}

export const ToolCard = React.memo(({ id, name, version, installed, path, detectedPath, configPath, skillsCount, installedSkillsCount, activeModel, website, iconBase64, names, selected = false, onClick }: ToolCardProps) => {
    const resolvedSkillsCount = skillsCount ?? installedSkillsCount ?? 0;
    const { t, locale } = useI18n();
    const displayName = (names && locale !== 'en' && names[locale]) || name;
    return (
        <div
            className={`p-5 border ${selected ? 'border-cyber-accent shadow-[0_0_10px_rgba(0,255,157,0.3)]' : 'border-cyber-border shadow-cyber-card'} relative overflow-hidden rounded-card ${installed ? 'cursor-pointer hover:bg-black/90' : 'cursor-default opacity-80'} transition-all bg-black/80 flex flex-col`}
            onClick={installed ? onClick : undefined}
        >
            {/* Tool icon top-right */}
            <img
                src={`./icons/tools/${id}.svg`}
                alt={name}
                className={`absolute top-4 right-4 w-10 h-10 rounded-lg ${selected ? 'opacity-100' : installed ? 'opacity-60' : 'opacity-20'}`}
                onError={(e) => {
                    const img = e.target as HTMLImageElement;
                    if (img.src.endsWith('.svg')) {
                        img.src = `./icons/tools/${id}.png`;
                    } else if (!img.src.startsWith('data:') && iconBase64) {
                        img.src = iconBase64;
                    } else {
                        img.style.display = 'none';
                    }
                }}
            />
            <div className={`text-lg font-bold truncate pr-12 ${installed ? 'text-cyber-accent' : 'text-cyber-text-secondary'}`}>{displayName}</div>
            <div className={`text-xs space-y-1.5 mt-3 ${installed ? 'text-cyber-accent/60' : 'text-cyber-text-muted/70'}`}>
                <div className="truncate">{t('tool.models')}: {installed ? (activeModel || '-') : '-'}</div>
                <div className="truncate">{t('tool.skills')}: {installed ? `${resolvedSkillsCount} ${t('tool.skillsInstalled')}` : '-'}</div>
                <div className="truncate">{t('tool.app')}: {installed ? (detectedPath || path || '-') : '-'}</div>
                <div className="truncate">{t('tool.config')}: {installed ? (configPath || '-') : '-'}</div>
            </div>
        </div>
    );
});
