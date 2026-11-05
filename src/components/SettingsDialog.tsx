// SettingsDialog — Global settings modal (gear button in sidebar)
import React, { useState, useEffect, useCallback, useRef } from 'react';
import {
  X,
  Globe,
  ExternalLink,
  Check,
  Palette,
  Settings2,
  Sparkles,
  Power,
  Monitor,
  Gift,
  CodeXml,
  ShieldCheck,
  Newspaper,
  Trophy,
  Server,
} from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import { ViewModeTabs } from './ViewModeTabs';
import { ViewModeTabGroup } from './ViewModeTabGroup';
import { useBackgroundStore } from '../stores/backgroundStore';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import { useThemeStore, type ThemeMode } from '../stores/themeStore';
import { COLOR_THEMES, type ColorThemeId } from '../data/colorThemes';
import { useUiScaleStore } from '../stores/uiScaleStore';
import { useEasterEggStore } from '../stores/easterEggStore';
import { useRewardClaimStore } from '../stores/rewardClaimStore';
import { RoutingToggle } from './RoutingToggle';
import { ToggleSwitch } from './ToggleSwitch';
import { useNavigationStore } from '../stores/navigationStore';
import {
  OPTIONAL_PAGE_SETTINGS,
  optionalPagesFromSettings,
  type OptionalPage,
} from '../stores/optionalPages';
import { useLocalServerStatusStore } from '../stores/localServerStatusStore';
import { useProtectionStore } from '../stores/protectionStore';
import { usePluginProductStore } from '../stores/pluginProductStore';

// All supported locales
const LOCALE_OPTIONS = [
  { id: 'en', glyph: 'A', label: 'English' },
  { id: 'zh-Hans', glyph: '简', label: '简体中文' },
  { id: 'zh-Hant', glyph: '繁', label: '繁體中文' },
  { id: 'ja', glyph: 'あ', label: '日本語' },
];

type SettingsTab = 'general' | 'appearance' | 'language';

const UI_SCALE_TICKS = [70, 80, 90, 100, 110, 120, 130, 140, 150];

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
  const [activeTab, setActiveTab] = useState<SettingsTab>('general');
  const [appVersion, setAppVersion] = useState<string>('');
  const [closeToTray, setCloseToTray] = useState<boolean | null>(false);
  const [settingsReady, setSettingsReady] = useState(false);
  const [protectionEnabled, setProtectionEnabled] = useState(false);
  const protectionPending = useProtectionStore((s) => s.pending);
  const setProtectionPending = (pending: boolean) => useProtectionStore.setState({ pending });
  const [protectionError, setProtectionError] = useState(false);
  const protectionChange = useRef({ revision: 0, pending: false });
  useEffect(
    () =>
      useProtectionStore.subscribe((state, previous) => {
        if (
          state.revision !== previous.revision &&
          state.enabled !== null &&
          !protectionChange.current.pending
        )
          setProtectionEnabled(state.enabled);
      }),
    []
  );
  const developerMode = useNavigationStore((state) => state.developerMode);
  const setDeveloperMode = useNavigationStore((state) => state.setDeveloperMode);
  const [developerPending, setDeveloperPending] = useState(false);
  const [developerError, setDeveloperError] = useState(false);
  const developerChange = useRef({ revision: 0, pending: false });
  const optionalPages = useNavigationStore((state) => state.optionalPages);
  const setOptionalPages = useNavigationStore((state) => state.setOptionalPages);
  const optionalChange = useRef({ pending: false });
  const [optionalPending, setOptionalPending] = useState<OptionalPage | null>(null);
  const [optionalError, setOptionalError] = useState(false);
  const easterEgg = useEasterEggStore((state) => state.enabled);
  const setEasterEgg = useEasterEggStore((state) => state.setEnabled);
  const autoClaimRewards = useRewardClaimStore((state) => state.enabled);
  const setAutoClaimRewards = useRewardClaimStore((state) => state.setEnabled);
  const background = useBackgroundStore((state) => state.mode);
  const setBackground = useBackgroundStore((state) => state.setMode);
  // Launch-at-startup toggle. Source of truth is the autostart plugin's OS
  // registration (isEnabled), queried each time the dialog opens, so changes
  // made outside the app (msconfig, Login Items) are reflected. Default off.
  const [launchAtStartup, setLaunchAtStartup] = useState(false);
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const colorTheme = useThemeStore((s) => s.colorTheme);
  const setColorTheme = useThemeStore((s) => s.setColorTheme);
  const dialogRef = useRef<HTMLDivElement>(null);
  const uiScale = useUiScaleStore();
  const scalePreference = uiScale.requestedPreference ?? uiScale.preference;
  const scalePercent =
    scalePreference === 'auto' ? Math.round(uiScale.scale * 100) : scalePreference;
  const [dragPercent, setDragPercent] = useState<number | null>(null);
  const sliderPercent = dragPercent ?? scalePercent;
  const scaleDrag = useRef<{
    screenX: number;
    percent: number;
    width: number;
    tick: number;
  } | null>(null);
  const changeScale = (percent: number) => {
    void uiScale.setPreference(percent);
  };
  const finishScaleDrag = () => {
    scaleDrag.current = null;
    setDragPercent(null);
  };
  if (dragPercent !== null && (!isOpen || activeTab !== 'general')) {
    setDragPercent(null);
  }

  useEffect(() => {
    // Removing the slider during a drag can skip its lost-pointer-capture handler.
    scaleDrag.current = null;
  }, [isOpen, activeTab]);

  // Read the installed binary version from Tauri at runtime — single source of truth (tauri.conf.json).
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(''));
  }, []);

  // Load backend settings each time the dialog opens.
  useEffect(() => {
    let active = true;
    const developerReadRevision = developerChange.current.revision;
    const optionalReadRevision = useNavigationStore.getState().optionalPagesRevision;
    const protectionReadRevision = protectionChange.current.revision;
    const sharedProtectionRevision = useProtectionStore.getState().revision;
    if (isOpen) {
      void Promise.resolve().then(() => {
        if (active) {
          setSettingsReady(false);
          setDeveloperError(false);
          setProtectionError(false);
        }
      });
      // Reflect the real OS autostart registration (may have been toggled
      // outside the app). Default to false if the query fails.
      isEnabled()
        .then(setLaunchAtStartup)
        .catch(() => {});
      api
        .getSettings()
        .then((settings) => {
          if (!active) return;
          // A reopened dialog can return an older snapshot after an explicit save.
          if (
            !developerChange.current.pending &&
            developerReadRevision === developerChange.current.revision
          ) {
            setDeveloperMode(settings.developerMode !== false);
          }
          if (
            !protectionChange.current.pending &&
            protectionReadRevision === protectionChange.current.revision &&
            sharedProtectionRevision === useProtectionStore.getState().revision
          ) {
            setProtectionEnabled(settings.sensitiveInfoProtection === true);
          }
          if (
            !optionalChange.current.pending &&
            optionalReadRevision === useNavigationStore.getState().optionalPagesRevision
          ) {
            setOptionalPages(optionalPagesFromSettings(settings));
          }
          setSettingsReady(true);
          // null ("always ask") is a real selectable value, not a missing
          // field — the backend stores it as Option<bool>=None. When the user
          // explicitly chose it (closeWindowBehaviorSet === true), preserve
          // null so the dialog reflects their choice instead of resetting to
          // "quit directly" (false). Only fall back to false for the genuinely
          // unset case (never configured), matching the original default.
          setCloseToTray(
            settings.closeWindowBehaviorSet && settings.closeToTray === null
              ? null
              : (settings.closeToTray ?? false)
          );
        })
        .catch(() => {
          if (active) {
            setDeveloperError(true);
            setProtectionError(true);
          }
        });
    }
    return () => {
      active = false;
    };
  }, [isOpen, setDeveloperMode, setOptionalPages]);

  const handleOptionalChange = async (page: OptionalPage, value: boolean) => {
    if (
      !settingsReady ||
      useProtectionStore.getState().pending ||
      developerChange.current.pending ||
      protectionChange.current.pending ||
      optionalChange.current.pending
    )
      return;
    optionalChange.current.pending = true;
    setOptionalPending(page);
    setProtectionPending(true);
    setOptionalError(false);
    try {
      const settings = await api.getSettings();
      await api.saveSettings({ ...settings, [OPTIONAL_PAGE_SETTINGS[page]]: value });
      setOptionalPages({ [page]: value });
      if (page === 'localLlm' && !value) {
        useLocalServerStatusStore.getState().setRunning(false);
        window.dispatchEvent(new Event('models-changed'));
      }
    } catch {
      setOptionalError(true);
    } finally {
      optionalChange.current.pending = false;
      setOptionalPending(null);
      setProtectionPending(false);
    }
  };

  const handleDeveloperChange = async (value: boolean) => {
    if (
      !settingsReady ||
      optionalChange.current.pending ||
      useProtectionStore.getState().pending ||
      developerChange.current.pending ||
      protectionChange.current.pending
    )
      return;
    developerChange.current.pending = true;
    developerChange.current.revision++;
    setProtectionPending(true);
    setDeveloperPending(true);
    setDeveloperError(false);
    try {
      const settings = await api.getSettings();
      await api.saveSettings({ ...settings, developerMode: value });
      setDeveloperMode(value);
    } catch {
      setDeveloperError(true);
    } finally {
      developerChange.current.pending = false;
      developerChange.current.revision++;
      setProtectionPending(false);
      setDeveloperPending(false);
    }
  };

  const handleProtectionChange = async (value: boolean) => {
    if (
      !settingsReady ||
      optionalChange.current.pending ||
      useProtectionStore.getState().pending ||
      protectionChange.current.pending ||
      developerChange.current.pending
    )
      return;
    protectionChange.current.pending = true;
    protectionChange.current.revision++;
    setProtectionPending(true);
    setProtectionError(false);
    try {
      const settings = await api.getSettings();
      await api.saveSettings({ ...settings, sensitiveInfoProtection: value });
      setProtectionEnabled(value);
      useProtectionStore.getState().publish(value);
      useProtectionStore.setState({ saveError: false });
    } catch {
      setProtectionError(true);
    } finally {
      protectionChange.current.pending = false;
      protectionChange.current.revision++;
      setProtectionPending(false);
    }
  };

  // Toggle launch-at-startup: register/unregister the OS autostart entry.
  // The entry launches with --minimized so a boot start stays in the tray.
  const handleLaunchAtStartupChange = useCallback((value: boolean) => {
    setLaunchAtStartup(value);
    if (value) {
      enable().catch(() => setLaunchAtStartup(false));
    } else {
      disable().catch(() => setLaunchAtStartup(true));
    }
  }, []);

  // Save closeToTray setting when it changes
  const handleCloseToTrayChange = useCallback(async (value: boolean | null) => {
    setCloseToTray(value);
    const settings = await api.getSettings();
    // Mark the close behavior as explicitly chosen. This both suppresses the
    // first-time onboarding dialog and — critically — prevents that dialog from
    // later overwriting an explicit "always ask" (null) selection.
    await api.saveSettings({
      ...settings,
      closeToTray: value,
      closeWindowBehaviorSet: true,
    });
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

  if (!isOpen) return null;

  return (
    <div
      className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${
        isAnimatingOut ? 'opacity-0' : 'opacity-100'
      }`}
    >
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/55" onClick={handleClose} />

      {/* Dialog */}
      <div
        ref={dialogRef}
        className={`relative flex h-[72vh] w-[clamp(800px,62vw,1000px)] max-w-[92vw] overflow-hidden rounded-xl border border-cyber-border/30 bg-cyber-bg shadow-2xl transition-all duration-200 ${
          isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        <aside className="flex w-[168px] flex-shrink-0 flex-col p-3">
          <div className="px-3 pt-2 pb-4 text-[16px] font-semibold tracking-wide text-cyber-text-muted">
            {t('settings.title')}
          </div>
          <nav className="space-y-1">
            {(
              [
                ['general', Settings2, t('settings.general')],
                ['appearance', Palette, t('settings.appearance')],
                ['language', Globe, t('settings.language')],
              ] as const
            ).map(([id, Icon, label]) => (
              <button
                key={id}
                type="button"
                onClick={() => setActiveTab(id)}
                className={`flex h-10 w-full items-center gap-2.5 rounded-md px-3 text-[14px] transition-colors ${
                  activeTab === id
                    ? 'bg-cyber-elevated font-semibold text-cyber-text'
                    : 'text-cyber-text-secondary hover:bg-cyber-elevated/60 hover:text-cyber-text'
                }`}
              >
                <Icon size={16} />
                {label}
              </button>
            ))}
          </nav>

          <div className="mt-auto border-t border-cyber-border/50 px-3 pt-3">
            <div className="mb-2 text-[12px] font-mono text-cyber-text-muted">
              {appVersion ? `v${appVersion}` : '—'}
            </div>
            <button
              type="button"
              onClick={() => api.openExternal('https://echobird.ai')}
              className="flex items-center gap-1.5 text-[13px] font-medium text-cyber-text-secondary transition-colors hover:text-cyber-text"
            >
              EchoBird <ExternalLink size={12} />
            </button>
          </div>
        </aside>

        <section className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-[58px] flex-shrink-0 items-center justify-between border-b border-cyber-border/50 px-5">
            <span className="text-[16px] font-semibold text-cyber-text">
              {t(`settings.${activeTab}`)}
            </span>
            <button
              type="button"
              onClick={handleClose}
              className="text-cyber-text-secondary transition-colors close-button"
            >
              <X size={18} />
            </button>
          </header>

          {/* Content */}
          <div className="page-scroll flex-1 overflow-y-auto px-5 py-5">
            {activeTab === 'general' ? (
              <div className="space-y-5">
                <div className="space-y-2.5">
                  <div className="flex items-center justify-between gap-4">
                    <label
                      htmlFor="ui-scale"
                      className="flex items-center gap-2 text-[14px] font-medium text-cyber-text-secondary"
                    >
                      <Monitor size={14} />
                      {t('settings.uiScale')}
                    </label>
                    <div className="flex items-center gap-3">
                      <button
                        type="button"
                        aria-pressed={scalePreference === 'auto'}
                        onClick={() => void uiScale.setPreference('auto')}
                        className={`rounded px-2.5 py-1 text-[13px] transition-colors ${scalePreference === 'auto' ? 'bg-cyber-accent/10 text-cyber-accent' : 'text-cyber-text-secondary hover:bg-cyber-elevated'}`}
                      >
                        {t('settings.uiScaleAuto')}
                      </button>
                      <span className="w-12 text-right text-[14px] tabular-nums text-cyber-text">
                        {scalePercent}%
                      </span>
                    </div>
                  </div>
                  <div>
                    <input
                      id="ui-scale"
                      type="range"
                      min={70}
                      max={150}
                      step="any"
                      value={sliderPercent}
                      aria-valuetext={`${scalePercent}%`}
                      className="ui-scale-slider relative block w-full"
                      style={
                        {
                          '--ui-scale-progress': `${((sliderPercent - 70) / 80) * 100}%`,
                        } as React.CSSProperties
                      }
                      onChange={(event) => {
                        if (!scaleDrag.current) changeScale(Number(event.target.value));
                      }}
                      onKeyDown={(event) => {
                        let percent: number;
                        switch (event.key) {
                          case 'ArrowRight':
                          case 'ArrowUp':
                          case 'PageUp':
                            percent = scalePercent + 10;
                            break;
                          case 'ArrowLeft':
                          case 'ArrowDown':
                          case 'PageDown':
                            percent = scalePercent - 10;
                            break;
                          case 'Home':
                            percent = 70;
                            break;
                          case 'End':
                            percent = 150;
                            break;
                          default:
                            return;
                        }
                        event.preventDefault();
                        if (!scaleDrag.current) changeScale(percent);
                      }}
                      onPointerDown={(event) => {
                        if (event.button !== 0) return;
                        event.preventDefault();
                        event.currentTarget.focus();
                        event.currentTarget.setPointerCapture(event.pointerId);
                        const rect = event.currentTarget.getBoundingClientRect();
                        const width = rect.width - 14;
                        const percent = Math.max(
                          70,
                          Math.min(150, 70 + ((event.clientX - rect.left - 7) / width) * 80)
                        );
                        const tick = Math.round(percent / 10) * 10;
                        // Keep a screen-space baseline while zoom and window bounds move the track.
                        scaleDrag.current = {
                          screenX: event.screenX,
                          percent,
                          width: width * uiScale.scale,
                          tick,
                        };
                        setDragPercent(percent);
                        changeScale(tick);
                      }}
                      onPointerMove={(event) => {
                        const drag = scaleDrag.current;
                        if (drag) {
                          const percent = Math.max(
                            70,
                            Math.min(
                              150,
                              drag.percent + ((event.screenX - drag.screenX) / drag.width) * 80
                            )
                          );
                          setDragPercent(percent);
                          // The thumb moves freely; only crossing a tick changes the UI/window.
                          const tick =
                            percent >= drag.tick
                              ? Math.floor(percent / 10) * 10
                              : Math.ceil(percent / 10) * 10;
                          if (tick !== drag.tick) {
                            drag.tick = tick;
                            changeScale(tick);
                          }
                        }
                      }}
                      onPointerUp={finishScaleDrag}
                      onPointerCancel={finishScaleDrag}
                      onLostPointerCapture={finishScaleDrag}
                    />
                    <div
                      aria-hidden={true}
                      data-scale-ticks={true}
                      className="pointer-events-none relative mx-[7px] mt-1 h-2"
                    >
                      {UI_SCALE_TICKS.map((tick) => (
                        <span
                          key={tick}
                          data-scale-tick={tick}
                          style={{ left: `${((tick - 70) / 80) * 100}%` }}
                          className={`absolute top-0 w-0.5 -translate-x-1/2 rounded-full ${tick === 100 ? 'h-1.5 bg-cyber-text-secondary/70' : 'h-1 bg-cyber-text-muted/45'}`}
                        />
                      ))}
                    </div>
                    <div className="relative mx-[7px] h-4 text-[12px] leading-4 tabular-nums text-cyber-text-muted">
                      <span className="absolute left-0">70%</span>
                      <span className="absolute left-[37.5%] -translate-x-1/2 text-cyber-text-secondary">
                        100%
                      </span>
                      <span className="absolute right-0">150%</span>
                    </div>
                  </div>
                  {uiScale.failed && (
                    <p role="alert" className="text-[13px] text-red-400">
                      {t('settings.uiScaleError')}
                    </p>
                  )}
                </div>

                <div className="h-px bg-cyber-border/50" />

                <div className="space-y-2.5">
                  <div className="flex items-center gap-2">
                    <X size={14} className="text-cyber-text-secondary" />
                    <span className="text-[14px] font-medium text-cyber-text-secondary">
                      {t('settings.closeWindowBehavior')}
                    </span>
                  </div>
                  <ViewModeTabGroup className="view-mode-tabs settings-segment-tabs flex border border-cyber-border rounded-button overflow-hidden">
                    {(
                      [
                        [false, t('settings.closeDirectly')],
                        [true, t('settings.closeToTray')],
                        [null, t('settings.alwaysAsk')],
                      ] as const
                    ).map(([value, label]) => (
                      <button
                        key={String(value)}
                        onClick={() => handleCloseToTrayChange(value)}
                        aria-pressed={closeToTray === value}
                        className={`view-mode-tab flex-1 h-11 flex items-center justify-center text-[13px] transition-[color] ${
                          closeToTray === value
                            ? 'bg-cyber-elevated text-cyber-text font-semibold'
                            : 'text-cyber-text-secondary hover:text-cyber-text'
                        }`}
                      >
                        {label}
                      </button>
                    ))}
                  </ViewModeTabGroup>
                </div>

                <div className="h-px bg-cyber-border/50" />

                <div className="grid grid-cols-1 min-[700px]:grid-cols-2 min-[880px]:grid-cols-3 gap-x-4 gap-y-6">
                  <div className="flex items-center justify-between px-2">
                    <div className="flex items-center gap-2">
                      <Power size={14} className="text-cyber-text-secondary" />
                      <span className="text-[14px] font-medium text-cyber-text-secondary">
                        {t('settings.launchAtStartup')}
                      </span>
                    </div>
                    <ToggleSwitch
                      checked={launchAtStartup}
                      onChange={handleLaunchAtStartupChange}
                    />
                  </div>
                  <div className="flex items-center justify-between px-2">
                    <div className="flex items-center gap-2">
                      <Gift size={14} className="text-cyber-text-secondary" />
                      <span className="text-[14px] font-medium text-cyber-text-secondary">
                        {t('settings.autoClaimRewards')}
                      </span>
                    </div>
                    <ToggleSwitch checked={autoClaimRewards} onChange={setAutoClaimRewards} />
                  </div>
                  <div className="px-2">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <ShieldCheck size={14} className="text-cyber-text-secondary" />
                        <span className="text-[14px] font-medium text-cyber-text-secondary">
                          {t('settings.sensitiveInfoProtection')}
                        </span>
                      </div>
                      <RoutingToggle
                        label={t('settings.sensitiveInfoProtection')}
                        hideLabel
                        dimWhenDisabled={!settingsReady}
                        className="flex-row-reverse gap-1.5"
                        hint={t('settings.sensitiveInfoProtectionHint')}
                        checked={protectionEnabled}
                        disabled={!settingsReady || developerPending}
                        pending={protectionPending}
                        onChange={(value) => void handleProtectionChange(value)}
                      />
                    </div>
                    {protectionError && (
                      <p role="alert" className="mt-1 text-xs text-red-400">
                        {t('settings.sensitiveInfoProtectionError')}
                      </p>
                    )}
                  </div>
                  <div className="flex items-center justify-between gap-2 px-2">
                    <div className="flex items-center gap-2">
                      <Newspaper size={14} className="shrink-0 text-cyber-text-secondary" />
                      <span className="text-[14px] font-medium text-cyber-text-secondary">
                        {t('nav.aiNews')}
                      </span>
                    </div>
                    <RoutingToggle
                      label={t('nav.aiNews')}
                      hideLabel
                      dimWhenDisabled={!settingsReady}
                      checked={optionalPages.aiNews}
                      disabled={!settingsReady || developerPending || protectionPending}
                      pending={optionalPending === 'aiNews'}
                      onChange={(value) => void handleOptionalChange('aiNews', value)}
                    />
                  </div>
                  <div className="flex items-center justify-between gap-2 px-2">
                    <div className="flex items-center gap-2">
                      <Trophy size={14} className="shrink-0 text-cyber-text-secondary" />
                      <span className="text-[14px] font-medium text-cyber-text-secondary">
                        {t('nav.aiCareer')}
                      </span>
                    </div>
                    <RoutingToggle
                      label={t('nav.aiCareer')}
                      hideLabel
                      dimWhenDisabled={!settingsReady}
                      checked={optionalPages.aiCareer}
                      disabled={!settingsReady || developerPending || protectionPending}
                      pending={optionalPending === 'aiCareer'}
                      onChange={(value) => void handleOptionalChange('aiCareer', value)}
                    />
                  </div>
                  <div className="px-2">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <CodeXml size={14} className="text-cyber-text-secondary" />
                        <span className="text-[14px] font-medium text-cyber-text-secondary">
                          {t('settings.developerMode')}
                        </span>
                      </div>
                      <RoutingToggle
                        label={t('settings.developerMode')}
                        hideLabel
                        dimWhenDisabled={!settingsReady}
                        className="flex-row-reverse gap-1.5"
                        hint={t('settings.developerModeHint')}
                        checked={developerMode}
                        disabled={!settingsReady || protectionPending}
                        pending={developerPending}
                        onChange={(value) => void handleDeveloperChange(value)}
                      />
                    </div>
                    {developerError && (
                      <p role="alert" className="mt-1 text-xs text-red-400">
                        {t('settings.developerModeError')}
                      </p>
                    )}
                  </div>
                  <div className="flex items-center justify-between gap-2 px-2">
                    <div className="flex items-center gap-2">
                      <Server size={14} className="shrink-0 text-cyber-text-secondary" />
                      <span className="text-[14px] font-medium text-cyber-text-secondary">
                        {t('nav.localServer')}
                      </span>
                    </div>
                    <RoutingToggle
                      label={t('nav.localServer')}
                      hideLabel
                      dimWhenDisabled={!settingsReady}
                      checked={optionalPages.localLlm}
                      disabled={!settingsReady || developerPending || protectionPending}
                      pending={optionalPending === 'localLlm'}
                      onChange={(value) => void handleOptionalChange('localLlm', value)}
                    />
                  </div>
                  <div className="flex items-center justify-between px-2">
                    <div className="flex items-center gap-2">
                      <Sparkles size={14} className="text-cyber-text-secondary" />
                      <span className="text-[14px] font-medium text-cyber-text-secondary">
                        {t('settings.easterEgg')}
                      </span>
                    </div>
                    <ToggleSwitch checked={easterEgg} onChange={setEasterEgg} />
                  </div>
                </div>
                {optionalError && (
                  <p role="alert" className="text-xs text-red-400">
                    {t('settings.optionalFeaturesError')}
                  </p>
                )}
              </div>
            ) : activeTab === 'appearance' ? (
              <div className="space-y-5">
                <div className="flex flex-wrap items-center justify-between gap-4">
                  <span className="text-[12px] font-semibold text-cyber-text-secondary">
                    {t('settings.background')}
                  </span>
                  <ViewModeTabs
                    value={background}
                    onChange={setBackground}
                    options={[
                      { value: 'none', label: t('settings.backgroundNone') },
                      { value: 'characters', label: t('settings.backgroundCharacters') },
                      { value: 'carbon', label: t('settings.backgroundCarbon') },
                      { value: 'monogram', label: t('settings.backgroundMonogram') },
                    ]}
                  />
                </div>

                <div className="h-px bg-cyber-border/50" />

                <div className="flex items-center justify-between gap-4">
                  <span className="text-[12px] font-semibold text-cyber-text-secondary">
                    {t('settings.colorTheme')}
                  </span>
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

                <ColorThemePicker
                  value={colorTheme}
                  mode={themeMode}
                  locale={locale}
                  onChange={(id) => {
                    if (usePluginProductStore.getState().snapshot.theme) {
                      void usePluginProductStore
                        .getState()
                        .manage(null, 'theme', '')
                        .then(() => {
                          useThemeStore.getState().setPluginTheme(null);
                          setColorTheme(id);
                        })
                        .catch(() => {});
                    } else setColorTheme(id);
                  }}
                />
              </div>
            ) : (
              <div className="grid grid-cols-2 gap-2">
                {LOCALE_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    lang={option.id}
                    aria-pressed={locale === option.id}
                    onClick={() => onLocaleChange(option.id)}
                    className={`flex min-h-12 items-center gap-2.5 rounded-lg border-2 px-[11px] py-[9px] text-[13px] transition-colors ${locale === option.id ? 'border-cyber-accent/70 bg-cyber-accent/10 text-cyber-text' : 'border-transparent text-cyber-text-secondary hover:bg-cyber-elevated/60 hover:text-cyber-text'}`}
                  >
                    <span
                      aria-hidden="true"
                      className="w-[22px] flex-shrink-0 text-center text-[15px]"
                    >
                      {option.glyph}
                    </span>
                    <span className="flex-1 text-left">{option.label}</span>
                    {locale === option.id && <Check aria-hidden="true" size={14} />}
                  </button>
                ))}
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
};

// Compact segmented tabs for the theme mode: Light / Dark / System.
const ThemeSegmented: React.FC<{
  value: ThemeMode;
  onChange: (mode: ThemeMode) => void;
  labels: { light: string; dark: string; system: string };
}> = ({ value, onChange, labels }) => {
  const opts: Array<{ id: ThemeMode; label: string }> = [
    { id: 'light', label: labels.light },
    { id: 'dark', label: labels.dark },
    { id: 'system', label: labels.system },
  ];
  return (
    <ViewModeTabGroup className="view-mode-tabs settings-segment-tabs flex flex-shrink-0 border border-cyber-border rounded-button overflow-hidden">
      {opts.map((o) => {
        const active = value === o.id;
        return (
          <button
            key={o.id}
            type="button"
            onClick={() => onChange(o.id)}
            aria-pressed={active}
            className={`view-mode-tab whitespace-nowrap px-3 py-1.5 text-[12px] font-semibold transition-[color] ${
              active
                ? 'bg-cyber-elevated text-cyber-text'
                : 'text-cyber-text-muted hover:text-cyber-text-secondary'
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </ViewModeTabGroup>
  );
};

const ColorThemePicker: React.FC<{
  value: ColorThemeId;
  mode: ThemeMode;
  locale: string;
  onChange: (theme: ColorThemeId) => void;
}> = ({ value, mode, locale, onChange }) => (
  <div className="grid max-h-[360px] grid-cols-6 gap-x-3 gap-y-3.5 overflow-y-auto pr-1">
    {COLOR_THEMES.map((theme) => {
      const active = value === theme.id;
      const label = locale.startsWith('zh')
        ? theme.labelZh
        : locale === 'ja'
          ? theme.labelJa
          : theme.labelEn;
      return (
        <button
          key={theme.id}
          type="button"
          aria-pressed={active}
          aria-label={label}
          onClick={() => onChange(theme.id)}
          className="flex min-w-0 flex-col gap-1.5 bg-transparent"
        >
          <span
            className={`relative flex h-[46px] w-full overflow-hidden rounded-lg border-2 ${
              active ? 'border-cyber-accent' : 'border-transparent'
            }`}
          >
            <span
              className="block h-full flex-1"
              style={{
                backgroundImage: `linear-gradient(135deg, ${mode === 'dark' ? theme.dark.canvas : theme.light.canvas}, ${mode === 'dark' ? theme.dark.tertiary : theme.light.tertiary})`,
              }}
            />
            <span
              className="block h-full flex-1"
              style={{
                backgroundImage: `linear-gradient(135deg, ${mode === 'system' ? theme.dark.canvas : mode === 'dark' ? theme.dark.tertiary : theme.light.tertiary}, ${mode === 'system' ? theme.dark.tertiary : mode === 'dark' ? theme.dark.canvas : theme.light.canvas})`,
              }}
            />
            {active && (
              <span className="absolute top-1 right-1 flex h-[15px] w-[15px] items-center justify-center rounded-full bg-cyber-accent text-white">
                <Check size={9} strokeWidth={3.5} />
              </span>
            )}
          </span>
          <span
            className={`truncate text-center text-[11px] ${
              active ? 'text-cyber-text' : 'text-cyber-text-secondary'
            }`}
          >
            {label}
          </span>
        </button>
      );
    })}
  </div>
);
