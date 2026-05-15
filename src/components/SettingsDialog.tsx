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
  const [updateStatus, setUpdateStatus] = useState<
    'idle' | 'checking' | 'latest' | 'available' | 'error'
  >('idle');
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string>('');
  const [closeToTray, setCloseToTray] = useState<boolean | null>(false);
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const dialogRef = useRef<HTMLDivElement>(null);

  // Read the installed binary version from Tauri at runtime — single source of truth (tauri.conf.json).
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(''));
  }, []);

  // Load settings on mount
  useEffect(() => {
    if (isOpen) {
      api.getSettings().then((settings) => {
        setCloseToTray(settings.closeToTray ?? false);
      });
    }
  }, [isOpen]);

  // Save closeToTray setting when it changes
  const handleCloseToTrayChange = useCallback(async (value: boolean | null) => {
    setCloseToTray(value);
    const settings = await api.getSettings();
    await api.saveSettings({ ...settings, closeToTray: value });
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
      if (!res.ok) {
        setUpdateStatus('error');
        return;
      }
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

  // Reset status when dialog opens — fresh state each time the user
  // re-opens settings. Critical: depend ONLY on `isOpen`. Listing
  // `updateStatus` here makes the effect re-fire on every transition
  // (idle → checking → ...) and immediately snap status back to idle,
  // which is exactly the "check-for-updates button looks dead" bug:
  // the click *does* fire, but the effect clobbers the 'checking'
  // state before React can render it.
  useEffect(() => {
    if (isOpen) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setUpdateStatus('idle');
    }
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div
      className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${
        isAnimatingOut ? 'opacity-0' : 'opacity-100'
      }`}
    >
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={handleClose} />

      {/* Dialog */}
      <div
        ref={dialogRef}
        className={`relative w-[440px] max-w-[92vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden transition-all duration-200 ${
          isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top accent line */}
        <div className="h-px w-full bg-cyber-border" />

        {/* Header */}
        <div className="px-6 pt-5 pb-4 flex items-center justify-between">
          <span className="text-lg font-bold text-cyber-text">{t('settings.title')}</span>
          <button
            onClick={handleClose}
            className="text-cyber-text-secondary hover:text-cyber-text transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="px-6 pb-6 space-y-5">
          {/* Version */}
          <div className="flex items-center justify-between">
            <span className="text-[14px] text-cyber-text-secondary">{t('settings.version')}</span>
            <span className="text-[14px] font-mono font-medium text-cyber-text">
              {appVersion ? `v${appVersion}` : '—'}
            </span>
          </div>

          {/* Divider */}
          <div className="h-px bg-cyber-border/50" />

          {/* Appearance — Light / Dark / System */}
          <div className="space-y-2.5">
            <div className="flex items-center gap-2">
              <Sun size={14} className="text-cyber-text-secondary" />
              <span className="text-[14px] font-medium text-cyber-text-secondary">
                {t('settings.appearance')}
              </span>
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
          <div className="h-px bg-cyber-border/50" />

          {/* Close Window Behavior */}
          <div className="space-y-2.5">
            <div className="flex items-center gap-2">
              <X size={14} className="text-cyber-text-secondary" />
              <span className="text-[14px] font-medium text-cyber-text-secondary">
                {t('settings.closeWindowBehavior')}
              </span>
            </div>
            <div className="flex gap-1 p-1 bg-cyber-input border border-cyber-border rounded-button">
              <button
                onClick={() => handleCloseToTrayChange(false)}
                className={`flex-1 h-9 flex items-center justify-center text-[13px] transition-colors rounded ${
                  closeToTray === false
                    ? 'bg-cyber-text/15 text-cyber-text font-semibold'
                    : 'text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated'
                }`}
              >
                {t('settings.closeDirectly')}
              </button>
              <button
                onClick={() => handleCloseToTrayChange(true)}
                className={`flex-1 h-9 flex items-center justify-center text-[13px] transition-colors rounded ${
                  closeToTray === true
                    ? 'bg-cyber-text/15 text-cyber-text font-semibold'
                    : 'text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated'
                }`}
              >
                {t('settings.closeToTray')}
              </button>
              <button
                onClick={() => handleCloseToTrayChange(null)}
                className={`flex-1 h-9 flex items-center justify-center text-[13px] transition-colors rounded ${
                  closeToTray === null
                    ? 'bg-cyber-text/15 text-cyber-text font-semibold'
                    : 'text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated'
                }`}
              >
                {t('settings.alwaysAsk')}
              </button>
            </div>
          </div>

          {/* Divider */}
          <div className="h-px bg-cyber-border/50" />

          {/* Language */}
          <div className="space-y-2.5">
            <div className="flex items-center gap-2">
              <Globe size={14} className="text-cyber-text-secondary" />
              <span className="text-[14px] font-medium text-cyber-text-secondary">
                {t('settings.language')}
              </span>
            </div>
            <MiniSelect value={locale} onChange={onLocaleChange} options={LOCALE_OPTIONS} />
          </div>

          {/* Divider */}
          <div className="h-px bg-cyber-border/50" />

          {/* Update check */}
          <div className="space-y-2.5">
            <div className="flex items-center gap-2">
              <Download size={14} className="text-cyber-text-secondary" />
              <span className="text-[14px] font-medium text-cyber-text-secondary">
                {t('settings.updates')}
              </span>
            </div>

            <div className="h-10 flex items-center">
              {updateStatus === 'idle' && (
                <button
                  onClick={checkForUpdates}
                  className="w-full h-10 text-[14px] font-semibold border border-cyber-border/50 bg-cyber-input/50 text-cyber-text hover:bg-cyber-input hover:border-cyber-border transition-colors rounded-button"
                >
                  {t('settings.checkForUpdates')}
                </button>
              )}

              {updateStatus === 'checking' && (
                <div className="w-full h-10 flex items-center justify-center text-[14px] text-cyber-text-secondary border border-cyber-border/30 bg-cyber-input/30 rounded-button">
                  {t('settings.checking')}
                </div>
              )}

              {updateStatus === 'latest' && (
                <div className="w-full h-10 flex items-center justify-center gap-1.5 text-[14px] text-cyber-text border border-cyber-border/30 bg-cyber-input/30 rounded-button">
                  <span className="text-cyber-accent">✓</span> {t('settings.latestVersion')}
                </div>
              )}

              {updateStatus === 'available' && (
                <button
                  onClick={() => api.openExternal('https://echobird.ai/')}
                  className="flex items-center justify-center gap-1.5 w-full h-10 text-[14px] font-semibold border border-cyber-accent/50 bg-cyber-accent/10 text-cyber-accent hover:bg-cyber-accent/20 hover:border-cyber-accent transition-colors rounded-button"
                >
                  Update to v{latestVersion} <ExternalLink size={13} />
                </button>
              )}

              {updateStatus === 'error' && (
                <button
                  onClick={checkForUpdates}
                  className="w-full h-10 text-[14px] font-semibold border border-cyber-error/40 bg-cyber-error/5 text-cyber-error hover:bg-cyber-error/10 hover:border-cyber-error/60 transition-colors rounded-button"
                >
                  {t('settings.checkFailed')}
                </button>
              )}
            </div>
          </div>

          {/* Website link */}
          <div className="pt-2 flex justify-center">
            <button
              onClick={() => api.openExternal('https://echobird.ai')}
              className="text-[14px] font-mono font-medium text-cyber-text-secondary hover:text-cyber-text transition-colors flex items-center gap-1.5"
            >
              EchoBird <ExternalLink size={13} />
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
    { id: 'light', icon: <Sun size={14} />, label: labels.light },
    { id: 'dark', icon: <Moon size={14} />, label: labels.dark },
    { id: 'system', icon: <Monitor size={14} />, label: labels.system },
  ];
  return (
    <div className="flex gap-1 p-1 bg-cyber-input border border-cyber-border rounded-button">
      {opts.map((o) => {
        const active = value === o.id;
        return (
          <button
            key={o.id}
            onClick={() => onChange(o.id)}
            className={`flex-1 h-9 flex items-center justify-center gap-1.5 text-[14px] transition-colors rounded ${
              active
                ? 'bg-cyber-text/15 text-cyber-text font-semibold'
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
