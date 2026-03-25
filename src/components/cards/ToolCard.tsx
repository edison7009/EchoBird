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

    activeModel?: string;
    website?: string;
    iconBase64?: string;
    names?: Record<string, string>;  // i18n names
    command?: string;                // CLI command (used by backend for detection)
    hasRemoteInstall?: boolean;      // show AI Auto-Install button (driven by remote index.json)
    selected?: boolean;
    onClick?: () => void;
    onMotherAgentInstall?: () => void;
}

export const ToolCard = React.memo(({ id, name, version, installed, path, detectedPath, configPath, activeModel, website, iconBase64, names, command, hasRemoteInstall, selected = false, onClick, onMotherAgentInstall }: ToolCardProps) => {
    const { t, locale } = useI18n();
    const displayName = (names && locale !== 'en' && (names[locale] || names[locale.split('-')[0]] || Object.entries(names).find(([k]) => k.startsWith(locale.split('-')[0]))?.[1])) || name;

    // Show AI Auto-Install button based on remote index (not local command field)
    const showMotherInstall = !installed && !!hasRemoteInstall;

    const handleCardClick = () => {
        if (installed) onClick?.();
    };

    return (
        <div
            className={`p-5 min-h-[160px] border ${selected ? 'border-cyber-accent shadow-[0_0_10px_rgba(0,255,157,0.3)]' : 'border-cyber-border shadow-cyber-card'} relative overflow-hidden rounded-card ${installed ? 'cursor-pointer hover:bg-black/90' : 'cursor-default opacity-80'} transition-all bg-black/80 flex flex-col`}
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

            {/* 4 rows always rendered to hold card height; invisible for CLI-installable tools */}
            <div className="relative mt-3">
                <div className={`text-xs space-y-1.5 ${installed ? 'text-cyber-accent/60' : 'text-cyber-text-muted/70'} ${showMotherInstall ? 'invisible' : ''}`}>
                    <div className="truncate">{t('tool.models')}: {installed ? (activeModel || '-') : '-'}</div>
                    <div className="truncate">{t('tool.app')}: {installed ? (detectedPath || path || '-') : '-'}</div>
                    <div className="truncate">{t('tool.config')}: {installed ? (configPath || '-') : '-'}</div>
                    <div className="truncate">{t('tool.version')}: {installed ? (version || '-') : '-'}</div>
                </div>
                {showMotherInstall && (
                    <div className="absolute inset-0 flex items-center justify-center">
                        <button
                            onClick={(e) => { e.stopPropagation(); onMotherAgentInstall?.(); }}
                            className="py-1.5 px-5 text-xs font-bold rounded bg-cyber-accent/40 text-black hover:bg-cyber-accent/55 transition-all"
                        >
                            {t('agent.installViaMother')}
                        </button>
                    </div>
                )}
            </div>
        </div>
    );
});
