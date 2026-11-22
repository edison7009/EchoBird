// SettingsDialog — Global settings modal (gear button in sidebar)
import React, { useState, useEffect, useCallback, useRef } from 'react';
import {
  X,
  Globe,
  ExternalLink,
  Palette,
  Star,
  Sun,
  Moon,
  SunMoon,
  ArrowDownToLine,
  CircleHelp,
  Settings2,
  Sparkles,
  Power,
  Gift,
  ShieldCheck,
  Newspaper,
  Trophy,
  Server,
} from 'lucide-react';
import { AutoImportSettings } from './AutoImportSettings';
import { getVersion } from '@tauri-apps/api/app';
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart';
import { useBackgroundStore } from '../stores/backgroundStore';
import { useI18n } from '../hooks/useI18n';
import * as api from '../api/tauri';
import { useThemeStore, type ThemeMode } from '../stores/themeStore';
import { COLOR_THEME_GRID, getColorTheme, type ColorThemeId } from '../data/colorThemes';
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
import { LOCALE_OPTIONS } from '../i18n/locales';
import './SettingsDialog.css';

type SettingsTab = 'general' | 'appearance' | 'language';

const GITHUB_URL = 'https://github.com/edison7009/EchoBird';

// A few characters for the character-stream sample.
const CHARACTER_SAMPLE = '7f3a.kc>_ ai 0x1e be ~/ $ go::run 42 fn ok λ ⌁ ::';

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
  // Do not paint a provisional left-hand choice before the saved value arrives.
  // The first real canvas target snaps into place; explicit changes still slide.
  const [closeToTray, setCloseToTray] = useState<boolean | null | undefined>(undefined);
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
    const optionalReadRevision = useNavigationStore.getState().optionalPagesRevision;
    const protectionReadRevision = protectionChange.current.revision;
    const sharedProtectionRevision = useProtectionStore.getState().revision;
    if (isOpen) {
      void Promise.resolve().then(() => {
        if (active) {
          setSettingsReady(false);
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
          // Ignore stale protection reads after an explicit save.
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
            setProtectionError(true);
          }
        });
    }
    return () => {
      active = false;
    };
  }, [isOpen, setOptionalPages]);

  const handleOptionalChange = async (page: OptionalPage, value: boolean) => {
    if (
      !settingsReady ||
      useProtectionStore.getState().pending ||
      protectionChange.current.pending ||
      optionalChange.current.pending
    )
      return;
    optionalChange.current.pending = true;
    setOptionalPending(page);
    setProtectionPending(true);
    setOptionalError(false);
    try {
      await api.saveSettings({ [OPTIONAL_PAGE_SETTINGS[page]]: value });
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

  const handleProtectionChange = async (value: boolean) => {
    if (
      !settingsReady ||
      optionalChange.current.pending ||
      useProtectionStore.getState().pending ||
      protectionChange.current.pending
    )
      return;
    protectionChange.current.pending = true;
    protectionChange.current.revision++;
    setProtectionPending(true);
    setProtectionError(false);
    try {
      await api.saveSettings({ sensitiveInfoProtection: value });
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
    // Mark the close behavior as explicitly chosen. This both suppresses the
    // first-time onboarding dialog and — critically — prevents that dialog from
    // later overwriting an explicit "always ask" (null) selection.
    await api.saveSettings({
      closeToTray: value,
      closeWindowBehaviorSet: true,
    });
  }, []);

  // A click anywhere on a feature tile flips its switch, through the switch
  // itself so its disabled and pending rules still apply. Clicks on the switch,
  // or on the help glyph, keep their own meaning.
  const toggleFeatureTile = (event: React.MouseEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest('button, a, [role="switch"], [aria-label]')) return;
    event.currentTarget.querySelector<HTMLButtonElement>('[role="switch"]')?.click();
  };

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
      {/* Backdrop: dims the page so the dialog stands clear of the app's background. */}
      <div className="settings-backdrop absolute inset-0" onClick={handleClose} />

      {/* Dialog */}
      <div
        ref={dialogRef}
        className={`settings-dialog relative flex overflow-hidden rounded-xl border border-cyber-border bg-cyber-bg shadow-2xl transition-all duration-200 ${
          isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        <aside className="flex w-[196px] flex-shrink-0 flex-col gap-1 border-r border-cyber-border/60 bg-cyber-surface/50 p-3">
          <div className="px-2.5 pt-1 pb-3.5 text-[20px] font-bold leading-7 tracking-wide text-cyber-text">
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
                className={`settings-nav${activeTab === id ? ' is-active' : ''}`}
              >
                <Icon size={16} />
                {label}
              </button>
            ))}
          </nav>

          <div className="mt-auto grid gap-2.5">
            <button
              type="button"
              onClick={() => api.openExternal(GITHUB_URL)}
              className="settings-star"
            >
              <b>
                {t('settings.starOnGithub')}
                <Star size={12} aria-hidden="true" />
              </b>
              <small>{t('settings.starOnGithubHint')}</small>
            </button>
            <div className="flex items-center justify-between px-1 text-[12px] text-cyber-text-muted">
              <span className="font-mono">{appVersion ? `v${appVersion}` : '—'}</span>
              <button
                type="button"
                onClick={() => api.openExternal('https://echobird.ai')}
                className="flex items-center gap-1 transition-colors hover:text-cyber-text"
              >
                EchoBird <ExternalLink size={11} />
              </button>
            </div>
          </div>
        </aside>

        <section className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-[58px] flex-shrink-0 items-center justify-between border-b border-cyber-border/50 px-6">
            <h2 className="text-[20px] font-semibold leading-7 text-cyber-text">
              {t(`settings.${activeTab}`)}
            </h2>
            <button
              type="button"
              onClick={handleClose}
              className="text-cyber-text-secondary transition-colors close-button"
            >
              <X size={18} />
            </button>
          </header>

          {/* Content: each page opens at its top. */}
          <div key={activeTab} className="page-scroll flex-1 overflow-y-auto px-6 py-5">
            {activeTab === 'general' ? (
              <div className="space-y-6">
                <div>
                  <label htmlFor="ui-scale" className="settings-label">
                    {t('settings.uiScale')}
                  </label>
                  <div className="settings-tile settings-scale">
                    <div className="flex items-center gap-3">
                      <button
                        type="button"
                        aria-pressed={scalePreference === 'auto'}
                        onClick={() => void uiScale.setPreference('auto')}
                        className="settings-chip"
                      >
                        {t('settings.uiScaleAuto')}
                      </button>
                      <span className="settings-scale-value">{scalePercent}%</span>
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
                      <div className="relative mx-[7px] h-4 text-[13px] leading-4 tabular-nums text-cyber-text-muted">
                        <span className="absolute left-0">70%</span>
                        <span className="absolute left-[37.5%] -translate-x-1/2 text-cyber-text-secondary">
                          100%
                        </span>
                        <span className="absolute right-0">150%</span>
                      </div>
                    </div>
                  </div>
                  {uiScale.failed && (
                    <p role="alert" className="mt-2 text-[14px] text-red-400">
                      {t('settings.uiScaleError')}
                    </p>
                  )}
                </div>

                <div>
                  <div className="settings-label">{t('settings.closeWindowBehavior')}</div>
                  <div className="grid grid-cols-3 gap-2.5">
                    {(
                      [
                        [false, X, t('settings.closeDirectly')],
                        [true, ArrowDownToLine, t('settings.closeToTray')],
                        [null, CircleHelp, t('settings.alwaysAsk')],
                      ] as const
                    ).map(([value, Icon, label]) => (
                      <button
                        key={String(value)}
                        type="button"
                        onClick={() => handleCloseToTrayChange(value)}
                        aria-pressed={closeToTray === value}
                        className="settings-tile settings-big"
                      >
                        <Icon size={24} aria-hidden="true" />
                        <span>{label}</span>
                      </button>
                    ))}
                  </div>
                </div>

                <div>
                  <div className="settings-label">{t('settings.features')}</div>
                  <div className="settings-features grid grid-cols-4 gap-2.5">
                    <div
                      onClick={toggleFeatureTile}
                      className={`settings-tile settings-feature${launchAtStartup ? ' is-on' : ''}`}
                    >
                      <div className="settings-feature-label">
                        <Power size={17} />
                        <span>{t('settings.launchAtStartup')}</span>
                      </div>
                      <ToggleSwitch
                        checked={launchAtStartup}
                        onChange={handleLaunchAtStartupChange}
                      />
                    </div>
                    <div
                      onClick={toggleFeatureTile}
                      className={`settings-tile settings-feature${autoClaimRewards ? ' is-on' : ''}`}
                    >
                      <div className="settings-feature-label">
                        <Gift size={17} />
                        <span>{t('settings.autoClaimRewards')}</span>
                      </div>
                      <ToggleSwitch checked={autoClaimRewards} onChange={setAutoClaimRewards} />
                    </div>
                    <div
                      onClick={toggleFeatureTile}
                      className={`settings-tile settings-feature${protectionEnabled ? ' is-on' : ''}`}
                    >
                      <div className="settings-feature-label">
                        <ShieldCheck size={17} />
                        <span>{t('settings.sensitiveInfoProtection')}</span>
                      </div>
                      <RoutingToggle
                        label={t('settings.sensitiveInfoProtection')}
                        hideLabel
                        dimWhenDisabled={!settingsReady}
                        className="flex-row-reverse gap-1.5"
                        hint={t('settings.sensitiveInfoProtectionHint')}
                        checked={protectionEnabled}
                        disabled={!settingsReady}
                        pending={protectionPending}
                        onChange={(value) => void handleProtectionChange(value)}
                      />
                      {protectionError && (
                        <p role="alert" className="text-[14px] font-normal text-red-400">
                          {t('settings.sensitiveInfoProtectionError')}
                        </p>
                      )}
                    </div>
                    {(
                      [
                        ['aiNews', Newspaper, t('nav.aiNews')],
                        ['aiCareer', Trophy, t('nav.aiCareer')],
                        ['localLlm', Server, t('nav.localServer')],
                      ] as const
                    ).map(([page, Icon, label]) => (
                      <div
                        key={page}
                        onClick={toggleFeatureTile}
                        className={`settings-tile settings-feature${optionalPages[page] ? ' is-on' : ''}`}
                      >
                        <div className="settings-feature-label">
                          <Icon size={17} />
                          <span>{label}</span>
                        </div>
                        <RoutingToggle
                          label={label}
                          hideLabel
                          dimWhenDisabled={!settingsReady}
                          checked={optionalPages[page]}
                          disabled={!settingsReady || protectionPending}
                          pending={optionalPending === page}
                          onChange={(value) => void handleOptionalChange(page, value)}
                        />
                      </div>
                    ))}
                    <div
                      onClick={toggleFeatureTile}
                      className={`settings-tile settings-feature${easterEgg ? ' is-on' : ''}`}
                    >
                      <div className="settings-feature-label">
                        <Sparkles size={17} />
                        <span>{t('settings.easterEgg')}</span>
                      </div>
                      <ToggleSwitch checked={easterEgg} onChange={setEasterEgg} />
                    </div>
                  </div>
                  {optionalError && (
                    <p role="alert" className="mt-2 text-[14px] text-red-400">
                      {t('settings.optionalFeaturesError')}
                    </p>
                  )}
                </div>

                <div className="settings-tile px-2 py-4">
                  <AutoImportSettings />
                </div>
              </div>
            ) : activeTab === 'appearance' ? (
              <div className="space-y-6">
                <div>
                  <div className="settings-label">{t('settings.themeMode')}</div>
                  <div className="grid grid-cols-3 gap-2.5">
                    {(
                      [
                        ['light', Sun, t('settings.themeLight')],
                        ['dark', Moon, t('settings.themeDark')],
                        ['system', SunMoon, t('settings.themeSystem')],
                      ] as const
                    ).map(([mode, Icon, label]) => (
                      <button
                        key={mode}
                        type="button"
                        onClick={() => setThemeMode(mode)}
                        aria-pressed={themeMode === mode}
                        className="settings-tile settings-big"
                      >
                        <Icon size={24} aria-hidden="true" />
                        <span>{label}</span>
                      </button>
                    ))}
                  </div>
                </div>

                <div>
                  <div className="settings-label">{t('settings.background')}</div>
                  <div className="grid grid-cols-5 gap-2.5">
                    {(
                      [
                        ['none', t('settings.backgroundNone')],
                        ['characters', t('settings.backgroundCharacters')],
                        ['carbon', t('settings.backgroundCarbon')],
                        ['basalt', t('settings.backgroundBasalt')],
                        ['monogram', t('settings.backgroundMonogram')],
                      ] as const
                    ).map(([mode, label]) => (
                      <button
                        key={mode}
                        type="button"
                        onClick={() => setBackground(mode)}
                        aria-pressed={background === mode}
                        className="settings-tile settings-filled settings-background"
                      >
                        <span
                          aria-hidden="true"
                          className={`settings-background-sample is-${mode}`}
                        >
                          {mode === 'characters' ? CHARACTER_SAMPLE : null}
                        </span>
                        <span>{label}</span>
                      </button>
                    ))}
                  </div>
                </div>

                <div>
                  <div className="settings-label">{t('settings.colorTheme')}</div>
                  <ColorThemePicker
                    value={colorTheme}
                    mode={themeMode}
                    locale={locale}
                    onChange={setColorTheme}
                  />
                </div>
              </div>
            ) : (
              <div>
                <div className="settings-label">{t('settings.interfaceLanguage')}</div>
                <div className="grid grid-cols-4 gap-2.5">
                  {LOCALE_OPTIONS.map((option) => (
                    <button
                      key={option.id}
                      type="button"
                      lang={option.id}
                      aria-pressed={locale === option.id}
                      onClick={() => onLocaleChange(option.id)}
                      className="settings-tile settings-big h-[112px]"
                    >
                      <span aria-hidden="true" className="settings-big-glyph">
                        {option.glyph}
                      </span>
                      <span>{option.label}</span>
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
};

const ColorThemePicker: React.FC<{
  value: ColorThemeId;
  mode: ThemeMode;
  locale: string;
  onChange: (theme: ColorThemeId) => void;
}> = ({ value, mode, locale, onChange }) => {
  const { t } = useI18n();
  return (
    <div className="grid grid-cols-6 gap-2.5">
      {COLOR_THEME_GRID.flat().map((id) => {
        const theme = getColorTheme(id);
        const active = value === theme.id;
        const label = locale.startsWith('zh')
          ? theme.labelZh
          : locale === 'ja'
            ? theme.labelJa
            : t(`settings.theme.${id}`);
        return (
          <button
            key={theme.id}
            type="button"
            aria-pressed={active}
            aria-label={label}
            onClick={() => onChange(theme.id)}
            className="settings-tile settings-filled settings-swatch"
          >
            <span
              style={{
                backgroundImage:
                  mode === 'system'
                    ? `linear-gradient(135deg, ${theme.light.canvas} 0%, ${theme.light.tertiary} 35%, ${theme.dark.tertiary} 65%, ${theme.dark.canvas} 100%)`
                    : `linear-gradient(135deg, ${theme[mode].canvas} 0%, ${theme[mode].secondary} 45%, ${theme[mode].tertiary} 100%)`,
              }}
            />
            <span>{label}</span>
          </button>
        );
      })}
    </div>
  );
};
