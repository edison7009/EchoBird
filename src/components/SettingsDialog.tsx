// SettingsDialog — Global settings modal (gear button in title bar)
import React, { useState, useEffect, useCallback, useRef } from 'react';
import { X, Globe, Download, ExternalLink, Sun, Moon, Monitor } from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';
import { MiniSelect } from './MiniSelect';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import { isNewerVersion } from '../utils/version';
import { useThemeStore, type ThemeMode } from '../stores/themeStore';

// All supported locales
const LOCALE_OPTIONS = [
    { id: 'en', label: 'English' },
    { id: 'zh-Hans', label: '简体中文' },
];

interface SettingsDialogProps {
    isOpen: boolean;
    onClose: () => void;
    locale: string;
    onLocaleChange: (locale: string) => void;
}

export const SettingsDialog: React.FC<SettingsDialogProps> = ({
    isOpen,
    onClose,
    locale,
    onLocaleChange,
}) => {
    const { t } = useI18n();
    const [isAnimatingOut, setIsAnimatingOut] = useState(false);
    const [updateStatus, setUpdateStatus] = useState<'idle' | 'checking' | 'latest' | 'available' | 'error'>('idle');
    const [latestVersion, setLatestVersion] = useState<string | null>(null);
    const [appVersion, setAppVersion] = useState<string>('');
    const [closeBehavior, setCloseBehavior] = useState('ask');
    const themeMode = useThemeStore(s => s.mode);
    const setThemeMode = useThemeStore(s => s.setMode);
    const dialogRef = useRef<HTMLDivElement>(null);

    // Read the installed binary version from Tauri at runtime — single source of truth (tauri.conf.json).
    useEffect(() => {
        getVersion().then(setAppVersion).catch(() => setAppVersion(''));
    }, []);

    // Close with animation
    const handleClose = useCallback(() => {
        setIsAnimatingOut(true);
        setTimeout(() => {
            setIsAnimatingOut(false);
            onClose();
        }, 200);
    }, [onClose]);

    // ESC to close
    useEffect(() => {
        if (!isOpen) return;
        const handler = (e: KeyboardEvent) => {
            if (e.key === 'Escape') handleClose();
        };
        window.addEventListener('keydown', handler);
        return () => window.removeEventListener('keydown', handler);
    }, [isOpen, handleClose]);

    // Check for updates via public version API
    const checkForUpdates = useCallback(async () => {
        setUpdateStatus('checking');
        try {
            const res = await fetch('https://echobird.ai/api/version/index.json');
            if (!res.ok) { setUpdateStatus('error'); return; }
            const data = await res.json();
            if (data.version && appVersion && isNewerVersion(data.version, appVersion)) {
                setLatestVersion(data.version);
                setUpdateStatus('available');
            } else {
                setUpdateStatus('latest');
            }
        } catch {
            setUpdateStatus('error');
        }
    }, [appVersion]);

    // Reset status and load settings when dialog opens
    useEffect(() => {
        if (isOpen) {
            setUpdateStatus('idle');
            api.getSettings().then(s => {
                setCloseBehavior(s.closeBehavior || 'ask');
            }).catch(() => { });
        }
    }, [isOpen]);

    if (!isOpen) return null;

    return (
        <div
            className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${isAnimatingOut ? 'opacity-0' : 'opacity-100'
                }`}
        >
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                onClick={handleClose}
            />

            {/* Dialog */}
            <div
                ref={dialogRef}
                className={`relative w-[400px] max-w-[90vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden transition-all duration-200 ${isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
                    }`}
                onClick={e => e.stopPropagation()}
            >
                {/* Top accent line */}
                <div className="h-px w-full bg-cyber-border" />

                {/* Header */}
                <div className="px-5 pt-4 pb-3 flex items-center justify-between">
                    <span className="text-sm font-mono font-bold tracking-wider text-cyber-text">
                        {t('settings.title')}
                    </span>
                    <button
                        onClick={handleClose}
                        className="text-cyber-text-secondary hover:text-cyber-text transition-colors"
                    >
                        <X size={14} />
                    </button>
                </div>

                {/* Content */}
                <div className="px-5 pb-5 space-y-5">

                    {/* Version */}
                    <div className="flex items-center justify-between">
                        <span className="text-xs font-mono text-cyber-text-secondary tracking-wider">{t('settings.version')}</span>
                        <span className="text-xs font-mono text-cyber-text">{appVersion ? `v${appVersion}` : '—'}</span>
                    </div>

                    {/* Divider */}
                    <div className="h-px bg-cyber-border" />

                    {/* Appearance — Light / Dark / System */}
                    <div className="space-y-2">
                        <div className="flex items-center gap-2">
                            <Sun size={12} className="text-cyber-text-secondary" />
                            <span className="text-xs font-mono text-cyber-text-secondary tracking-wider">{t('settings.appearance')}</span>
                        </div>
                        <ThemeSegmented
                            value={themeMode}
                            onChange={setThemeMode}
                            labels={{
                                light: t('settings.themeLight'),
                                dark: t('settings.themeDark'),
                                system: t('settings.themeSystem'),
                            }}
                        />
                    </div>

                    {/* Divider */}
                    <div className="h-px bg-cyber-border" />

                    {/* Language */}
                    <div className="space-y-2">
                        <div className="flex items-center gap-2">
                            <Globe size={12} className="text-cyber-text-secondary" />
                            <span className="text-xs font-mono text-cyber-text-secondary tracking-wider">{t('settings.language')}</span>
                        </div>
                        <MiniSelect
                            value={locale}
                            onChange={onLocaleChange}
                            options={LOCALE_OPTIONS}
                        />
                    </div>

                    {/* Divider */}
                    <div className="h-px bg-cyber-border" />

                    {/* Close behavior */}
                    <div className="space-y-2">
                        <div className="flex items-center gap-2">
                            <X size={12} className="text-cyber-text-secondary" />
                            <span className="text-xs font-mono text-cyber-text-secondary tracking-wider">{t('settings.closeBehavior')}</span>
                        </div>
                        <MiniSelect
                            value={closeBehavior}
                            onChange={(val) => {
                                setCloseBehavior(val);
                                api.getSettings().then(s => {
                                    api.saveSettings({ ...s, closeBehavior: val === 'ask' ? undefined : val }).catch(() => { });
                                }).catch(() => { });
                            }}
                            options={[
                                { id: 'ask', label: t('settings.closeAsk') },
                                { id: 'minimize', label: t('settings.closeMinimize') },
                                { id: 'quit', label: t('settings.closeQuit') },
                            ]}
                        />
                    </div>

                    {/* Divider */}
                    <div className="h-px bg-cyber-border" />

                    {/* Update check */}
                    <div className="space-y-2">
                        <div className="flex items-center gap-2">
                            <Download size={12} className="text-cyber-text-secondary" />
                            <span className="text-xs font-mono text-cyber-text-secondary tracking-wider">{t('settings.updates')}</span>
                        </div>

                        <div className="h-9 flex items-center">
                            {updateStatus === 'idle' && (
                                <button
                                    onClick={checkForUpdates}
                                    className="w-full h-9 text-xs font-mono font-bold border border-cyber-border/40 text-cyber-text hover:bg-cyber-text/10 transition-colors tracking-wider rounded-button"
                                >
                                    {t('settings.checkForUpdates')}
                                </button>
                            )}

                            {updateStatus === 'checking' && (
                                <div className="w-full h-9 flex items-center justify-center text-xs font-mono text-cyber-text-secondary border border-cyber-border-secondary/30 rounded-button">
                                    {t('settings.checking')}
                                </div>
                            )}

                            {updateStatus === 'latest' && (
                                <div className="w-full h-9 flex items-center justify-center text-xs font-mono text-cyber-text border border-cyber-border/30 rounded-button">
                                    ✓ {t('settings.latestVersion')}
                                </div>
                            )}

                            {updateStatus === 'available' && (
                                <button
                                    onClick={() => api.openExternal('https://echobird.ai/')}
                                    className="flex items-center justify-center gap-1.5 w-full h-9 text-xs font-mono border border-cyber-border-secondary/30 text-cyber-text-secondary hover:bg-cyber-accent-secondary/10 transition-colors tracking-wider rounded-button"
                                >
                                    UPDATE TO v{latestVersion} <ExternalLink size={10} />
                                </button>
                            )}

                            {updateStatus === 'error' && (
                                <button
                                    onClick={checkForUpdates}
                                    className="w-full h-9 text-xs font-mono border border-red-400/30 text-red-400 hover:bg-red-400/10 transition-colors tracking-wider rounded-button"
                                >
                                    {t('settings.checkFailed')}
                                </button>
                            )}
                        </div>
                    </div>

                    {/* Website link */}
                    <div className="pt-1 flex justify-center">
                        <button
                            onClick={() => api.openExternal('https://echobird.ai')}
                            className="text-[13px] font-mono text-cyber-text-secondary/80 hover:text-cyber-text transition-colors tracking-wider flex items-center gap-1.5"
                        >
                            EchoBird <ExternalLink size={12} />
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};

// 3-button segmented control for the theme: Light / Dark / System.
const ThemeSegmented: React.FC<{
    value: ThemeMode;
    onChange: (mode: ThemeMode) => void;
    labels: { light: string; dark: string; system: string };
}> = ({ value, onChange, labels }) => {
    const opts: Array<{ id: ThemeMode; icon: React.ReactNode; label: string }> = [
        { id: 'light', icon: <Sun size={12} />, label: labels.light },
        { id: 'dark', icon: <Moon size={12} />, label: labels.dark },
        { id: 'system', icon: <Monitor size={12} />, label: labels.system },
    ];
    return (
        <div className="flex gap-1 p-1 bg-cyber-input border border-cyber-border rounded-button">
            {opts.map(o => {
                const active = value === o.id;
                return (
                    <button
                        key={o.id}
                        onClick={() => onChange(o.id)}
                        className={`flex-1 h-7 flex items-center justify-center gap-1.5 text-xs font-mono transition-colors rounded ${
                            active
                                ? 'bg-cyber-text/15 text-cyber-text'
                                : 'text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated'
                        }`}
                    >
                        {o.icon}
                        {o.label}
                    </button>
                );
            })}
        </div>
    );
};
