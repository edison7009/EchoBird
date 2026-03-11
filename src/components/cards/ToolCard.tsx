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
    command?: string;                // CLI command (non-empty = installable via Mother Agent)
    selected?: boolean;
    onClick?: () => void;
    onMotherAgentInstall?: () => void;
}

export const ToolCard = React.memo(({ id, name, version, installed, path, detectedPath, configPath, skillsCount, installedSkillsCount, activeModel, website, iconBase64, names, command, selected = false, onClick, onMotherAgentInstall }: ToolCardProps) => {
    const resolvedSkillsCount = skillsCount ?? installedSkillsCount ?? 0;
    const { t, locale } = useI18n();
    const displayName = (names && locale !== 'en' && names[locale]) || name;

    // Uninstalled CLI tool → show Mother Agent install button
    const showMotherInstall = !installed && !!command;

    const handleCardClick = () => {
        if (showMotherInstall) {
            onMotherAgentInstall?.();
        } else if (installed) {
            onClick?.();
        }
    };

    return (
        <div
            className={`p-5 min-h-[160px] border ${selected ? 'border-cyber-accent shadow-[0_0_10px_rgba(0,255,157,0.3)]' : 'border-cyber-border shadow-cyber-card'} relative overflow-hidden rounded-card ${installed || showMotherInstall ? 'cursor-pointer hover:bg-black/90' : 'cursor-default opacity-80'} transition-all bg-black/80 flex flex-col`}
            onClick={handleCardClick}
        >
            {/* Tool icon top-right */}
            <img
                src={`./icons/tools/${id}.svg`}
                alt={name}
                className={`absolute top-4 right-4 w-10 h-10 rounded-lg ${selected ? 'opacity-100' : installed ? 'opacity-60' : showMotherInstall ? 'opacity-30' : 'opacity-20'}`}
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
            <div className={`text-lg font-bold truncate pr-12 ${installed ? 'text-cyber-accent' : showMotherInstall ? 'text-cyber-text-secondary' : 'text-cyber-text-secondary'}`}>{displayName}</div>

            {/* Mutually exclusive: install button OR 4 info rows */}
            <div className="flex-1 mt-3 flex flex-col justify-center">
                {showMotherInstall ? (
                    <div className="flex items-center justify-center">
                        <button
                            onClick={(e) => { e.stopPropagation(); onMotherAgentInstall?.(); }}
                            className="py-1.5 px-5 text-xs font-bold rounded border border-cyber-accent-secondary bg-cyber-accent-secondary text-black hover:bg-cyber-accent-secondary/90 hover:shadow-[0_0_10px_rgba(0,212,255,0.35)] transition-all"
                        >
                            {t('agent.installViaMother')}
                        </button>
                    </div>
                ) : (
                    <div className={`text-xs space-y-1.5 ${installed ? 'text-cyber-accent/60' : 'text-cyber-text-muted/70'}`}>
                        <div className="truncate">{t('tool.models')}: {installed ? (activeModel || '-') : '-'}</div>
                        <div className="truncate">{t('tool.skills')}: {installed ? `${resolvedSkillsCount} ${t('tool.skillsInstalled')}` : '-'}</div>
                        <div className="truncate">{t('tool.app')}: {installed ? (detectedPath || path || '-') : '-'}</div>
                        <div className="truncate">{t('tool.config')}: {installed ? (configPath || '-') : '-'}</div>
                    </div>
                )}
            </div>
        </div>
    );
});
