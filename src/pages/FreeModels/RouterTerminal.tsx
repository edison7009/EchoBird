import { useEffect, useRef, useState } from 'react';
import * as api from '../../api/tauri';
import { RoutingToggle } from '../../components/RoutingToggle';
import { useDocumentVisible } from '../../hooks/useDocumentVisible';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useProtectionStore } from '../../stores/protectionStore';
import { useFreeModels } from './provider';
import { useTerminalFeed } from './useTerminalFeed';
import { animateTerminal, type TerminalPlayback } from './terminalCanvas';
import './RouterTerminal.css';

export function RouterTerminal() {
  const { t } = useI18n();
  const { routers, activeRouterId, selectRouter, groupPending } = useFreeModels();
  const configurationActive = useNavigationStore(
    (state) => state.activePage === 'freeModels' && state.smartAccessTab === 'configuration'
  );
  const visible = useDocumentVisible();
  const active = visible && configurationActive;
  const enabled = useProtectionStore((s) => s.enabled);
  const canvas = useRef<HTMLCanvasElement>(null);
  const playback = useRef<TerminalPlayback>({
    group: '',
    generation: 0,
    realtime: false,
    protection: false,
  });
  const { pending, error, realtime } = useTerminalFeed(activeRouterId, active, playback);
  useEffect(() => {
    playback.current.protection = enabled === true;
  }, [enabled]);
  const [ready, setReady] = useState(false);
  const saving = useProtectionStore((s) => s.pending);
  const saveError = useProtectionStore((s) => s.saveError);
  const [settingError, setSettingError] = useState(false);
  useEffect(() => {
    let alive = true;
    const revision = useProtectionStore.getState().revision;
    void api
      .getSettings()
      .then((settings) => {
        if (!alive) return;
        if (useProtectionStore.getState().revision === revision) {
          useProtectionStore.getState().publish(settings.sensitiveInfoProtection === true);
        }
        setReady(true);
      })
      .catch(() => {
        if (alive) setSettingError(true);
      });
    return () => {
      alive = false;
    };
  }, []);
  const toggle = async (value: boolean) => {
    if (!ready || useProtectionStore.getState().pending) return;
    useProtectionStore.setState({ pending: true, saveError: false });
    setSettingError(false);
    try {
      await api.saveSettings({ sensitiveInfoProtection: value });
      useProtectionStore.getState().publish(value);
    } catch {
      useProtectionStore.setState({ saveError: true });
    } finally {
      useProtectionStore.setState({ pending: false });
    }
  };
  useEffect(() => {
    if (!active || !canvas.current) return;
    return animateTerminal(
      canvas.current,
      pending,
      t('freeModels.terminal.protected'),
      t('freeModels.terminal.waiting'),
      playback
    );
  }, [active, pending, t]);
  return (
    <div className="router-terminal">
      <div className="router-terminal-toolbar">
        <div
          role="group"
          aria-label={t('freeModels.router.title')}
          className="router-terminal-groups"
        >
          {routers.map((router, index) => (
            <button
              key={router.id}
              type="button"
              aria-label={router.name}
              aria-pressed={router.id === activeRouterId}
              disabled={groupPending}
              onClick={() => void selectRouter(router.id)}
              className="view-mode-tab text-sm cjk-btn transition-[color]"
            >
              {router.id === activeRouterId ? <span>{router.name}</span> : index + 1}
            </button>
          ))}
        </div>
        <div className="router-terminal-controls">
          {!realtime && (
            <span role="status" className="router-terminal-demo-notice">
              {t('freeModels.terminal.demoNotice')}
            </span>
          )}
          <RoutingToggle
            className="router-terminal-protection"
            label={t('settings.sensitiveInfoProtection')}
            hint={t('settings.sensitiveInfoProtectionHint')}
            checked={enabled === true}
            disabled={!ready}
            pending={saving}
            onChange={(value) => void toggle(value)}
          />
        </div>
      </div>
      {(settingError || saveError) && (
        <p role="alert" className="text-xs text-red-400">
          {t('settings.sensitiveInfoProtectionError')}
        </p>
      )}
      {error && (
        <p role="status" className="text-xs text-cyber-text-muted">
          {t('error.requestFailed')}
        </p>
      )}
      <div className="router-terminal-screen">
        <canvas ref={canvas} aria-label={t('freeModels.layout.terminal')} />
      </div>
    </div>
  );
}
