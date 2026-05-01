// Sidebar navigation component
import { useState, useEffect } from 'react';
import { Box, Cpu, Server, Activity } from 'lucide-react';
import { NavItem } from './NavItem';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';

declare const __APP_EDITION__: string;
const isFullEdition = __APP_EDITION__ === 'full';

export type PageType = 'models' | 'apps' | 'localLlm' | 'mother';

interface SidebarProps {
    activePage: PageType;
    onPageChange: (page: PageType) => void;
    agentRunning?: boolean;
    motherBadge?: boolean;
    updateAvailable?: string | null;
    onSettingsClick?: () => void;
}

export const Sidebar = ({ activePage, onPageChange, agentRunning: _agentRunning = false, motherBadge = false, updateAvailable = null, onSettingsClick }: SidebarProps) => {
    const { t } = useI18n();
    // Poll local model server status
    const [serverRunning, setServerRunning] = useState(false);

    useEffect(() => {
        if (!isFullEdition) return;
        const check = async () => {
            try {
                const info = await api.getLlmServerInfo();
                const running = info?.running ?? false;
                setServerRunning(prev => prev === running ? prev : running);
            } catch { setServerRunning(prev => prev === false ? prev : false); }
        };
        check();
        const interval = setInterval(check, 2000);
        return () => clearInterval(interval);
    }, []);

    return (
        <nav className="w-64 flex flex-col px-6 pb-6">
            <div className="mb-6 tracking-wide flex items-center gap-2 overflow-hidden">
                <span
                    className={`flex-shrink-0 ${activePage === 'mother' ? 'text-cyber-accent-secondary' : 'text-cyber-accent'}`}
                    style={{
                        fontFamily: "'JetBrains Mono', monospace",
                        fontWeight: 500,
                        letterSpacing: '0.5px',
                        fontSize: '15px',
                        lineHeight: '16px',
                    }}
                >
                    {'EchoBird.ai'}
                </span>
                {updateAvailable && (
                    <button
                        onClick={onSettingsClick}
                        className="flex-shrink-0 text-[12px] font-mono text-red-400 hover:opacity-70 transition-opacity animate-pulse leading-none"
                    >
                        {t('settings.updates')}
                    </button>
                )}
            </div>
            <div className="flex-1 space-y-6 text-sm">
                <NavItem
                    icon={<Box size={18} />}
                    label={t('nav.modelNexus')}
                    active={activePage === 'models'}
                    onClick={() => onPageChange('models')}
                />

                <NavItem
                    icon={<Cpu size={18} />}
                    label={t('nav.appManager')}
                    active={activePage === 'apps'}
                    onClick={() => onPageChange('apps')}
                />
                {isFullEdition && (
                    <NavItem
                        icon={<Server size={18} />}
                        label={t('nav.localServer')}
                        active={activePage === 'localLlm'}
                        onClick={() => onPageChange('localLlm')}
                    />
                )}
                <NavItem
                    icon={<Activity size={18} />}
                    label={t('nav.motherAgent')}
                    active={activePage === 'mother'}
                    onClick={() => onPageChange('mother')}
                    color="blue"
                    badge={motherBadge}
                />
            </div>

            {isFullEdition && (
                <div className="pt-4 text-[12px] text-cyber-text-secondary uppercase tracking-widest">
                    {t('nav.localServer')}: {serverRunning ? (
                        <span className="text-cyber-accent">{t('status.running')}</span>
                    ) : (
                        <span className="text-cyber-text-muted/70">{t('status.offline')}</span>
                    )}
                </div>
            )}


        </nav>
    );
};
