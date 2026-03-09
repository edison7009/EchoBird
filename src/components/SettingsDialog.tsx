// SettingsDialog — Global settings modal (gear button in title bar)
import React, { useState, useEffect, useCallback, useRef } from 'react';
import { X, Globe, Download, ExternalLink } from 'lucide-react';
import { MiniSelect } from './MiniSelect';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';

declare const __APP_VERSION__: string;
const APP_VERSION = __APP_VERSION__;

/** Returns true only if `remote` version is strictly greater than `local` (semver X.Y.Z) */
function isNewerVersion(remote: string, local: string): boolean {
    const parse = (v: string) => v.replace(/^v/, '').split('.').map(Number);
    const [rMaj, rMin, rPat] = parse(remote);
    const [lMaj, lMin, lPat] = parse(local);
    if (rMaj !== lMaj) return rMaj > lMaj;
    if (rMin !== lMin) return rMin > lMin;
    return rPat > lPat;
}

// All supported locales
const LOCALE_OPTIONS = [
    { id: 'en', label: 'English' },
    { id: 'zh-Hans', label: '简体中文' },
    { id: 'zh-Hant', label: '繁體中文' },
    { id: 'ja', label: '日本語' },
    { id: 'ko', label: '한국어' },
    { id: 'de', label: 'Deutsch' },
    { id: 'fr', label: 'Français' },
    { id: 'es', label: 'Español' },
    { id: 'pt', label: 'Português' },
    { id: 'it', label: 'Italiano' },
    { id: 'nl', label: 'Nederlands' },
    { id: 'ru', label: 'Русский' },
    { id: 'ar', label: 'العربية' },
    { id: 'hi', label: 'हिन्दी' },
    { id: 'bn', label: 'বাংলা' },
    { id: 'th', label: 'ไทย' },
    { id: 'vi', label: 'Tiếng Việt' },
    { id: 'id', label: 'Bahasa Indonesia' },
    { id: 'ms', label: 'Bahasa Melayu' },
    { id: 'tr', label: 'Türkçe' },
    { id: 'pl', label: 'Polski' },
    { id: 'cs', label: 'Čeština' },
    { id: 'hu', label: 'Magyar' },
    { id: 'sv', label: 'Svenska' },
    { id: 'fi', label: 'Suomi' },
    { id: 'el', label: 'Ελληνικά' },
    { id: 'he', label: 'עברית' },
    { id: 'fa', label: 'فارسی' },
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
    const [closeBehavior, setCloseBehavior] = useState('ask');
    const dialogRef = useRef<HTMLDivElement>(null);

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
            if (data.version && isNewerVersion(data.version, APP_VERSION)) {
                setLatestVersion(data.version);
                setUpdateStatus('available');
            } else {
                setUpdateStatus('latest');
            }
        } catch {
            setUpdateStatus('error');
        }
    }, []);

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
                className={`relative w-[400px] max-w-[90vw] border border-cyber-accent/30 bg-cyber-bg shadow-[0_0_30px_rgba(0,255,157,0.08)] rounded-xl overflow-hidden transition-all duration-200 ${isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
                    }`}
                onClick={e => e.stopPropagation()}
            >
                {/* Top accent line */}
                <div className="h-[2px] w-full bg-gradient-to-r from-cyber-accent/60 via-cyber-accent-secondary/40 to-transparent" />

                {/* Header */}
                <div className="px-5 pt-4 pb-3 flex items-center justify-between">
                    <span className="text-sm font-mono font-bold tracking-wider text-cyber-accent">
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
                        <span className="text-xs font-mono text-cyber-accent">v{APP_VERSION}</span>
                    </div>

                    {/* Divider */}
                    <div className="h-px bg-cyber-border" />

                    {/* Language */}
                    <div className="space-y-2">
                        <div className="flex items-center gap-2">
                            <Globe size={12} className="text-cyber-accent-secondary" />
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
                            <X size={12} className="text-cyber-accent-secondary" />
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
                            <Download size={12} className="text-cyber-accent-secondary" />
                            <span className="text-xs font-mono text-cyber-text-secondary tracking-wider">{t('settings.updates')}</span>
                        </div>

                        <div className="h-9 flex items-center">
                            {updateStatus === 'idle' && (
                                <button
                                    onClick={checkForUpdates}
                                    className="w-full h-9 text-xs font-mono font-bold border border-cyber-accent/40 text-cyber-accent hover:bg-cyber-accent/10 transition-colors tracking-wider rounded-button"
                                >
                                    {t('settings.checkForUpdates')}
                                </button>
                            )}

                            {updateStatus === 'checking' && (
                                <div className="w-full h-9 flex items-center justify-center text-xs font-mono text-cyber-accent-secondary border border-cyber-accent-secondary/30 rounded-button">
                                    {t('settings.checking')}
                                </div>
                            )}

                            {updateStatus === 'latest' && (
                                <div className="w-full h-9 flex items-center justify-center text-xs font-mono text-cyber-accent border border-cyber-accent/30 rounded-button">
                                    ✓ {t('settings.latestVersion')}
                                </div>
                            )}

                            {updateStatus === 'available' && (
                                <button
                                    onClick={() => api.openExternal('https://echobird.ai/')}
                                    className="flex items-center justify-center gap-1.5 w-full h-9 text-xs font-mono border border-cyber-accent-secondary/30 text-cyber-accent-secondary hover:bg-cyber-accent-secondary/10 transition-colors tracking-wider rounded-button"
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
                            className="text-[13px] font-mono text-cyber-text-secondary/80 hover:text-cyber-accent transition-colors tracking-wider flex items-center gap-1.5"
                        >
                            Echobird.ai <ExternalLink size={12} />
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};
