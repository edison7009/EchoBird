import { useEffect } from 'react';
import { LoaderCircle, Mic, Square } from 'lucide-react';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { showMainWindow, voiceIslandReady, type VoiceSnapshot } from '../../api/voiceInput';
import { useVoiceInputStore, voiceBusy } from '../../stores/voiceInputStore';
import {
  THEME_CHANGED_EVENT,
  THEME_REQUEST_EVENT,
  useThemeStore,
  type ThemeSelection,
} from '../../stores/themeStore';
import { useI18n } from '../../hooks/useI18n';
import { useSystemDark } from '../../hooks/useSystemDark';
import type { TKey } from '../../i18n';
import { VoiceAppIcon } from './VoiceAppIcon';
import { VoiceFlow } from './VoiceFlow';
import { voicePaletteColor } from './voicePalettes';
import './VoiceInput.css';

export function IslandContent({
  snapshot,
  preview = false,
}: {
  snapshot: VoiceSnapshot;
  preview?: boolean;
}) {
  const { t } = useI18n();
  const resolvedTheme = useThemeStore((theme) => theme.resolved);
  const systemDark = useSystemDark();
  const state = useVoiceInputStore();
  const { config } = snapshot;
  const islandTheme =
    config.islandTheme === 'system' ? (systemDark ? 'dark' : 'light') : config.islandTheme;
  const palette = config.islandPalette ?? 'white';
  const light = islandTheme === 'light' || (islandTheme === 'auto' && resolvedTheme === 'light');
  const phase = preview ? 'idle' : snapshot.phase;
  const recording = phase === 'recording';
  const ready = !voiceBusy(phase);
  // Connecting before listening and waiting for the result both show the spinner and take no clicks.
  const waiting = !preview && (state.pending || (!recording && !ready));
  const error = !preview && (snapshot.error || state.error);
  const idle = phase === 'idle' || phase === 'done';
  const speaking = ['recording', 'testing'].includes(snapshot.phase);
  const level = preview || idle ? 0.65 : 0;
  const flowing = !error && (preview || idle || recording);
  return (
    <div
      className={'voice-island voice-theme-' + islandTheme}
      data-active={flowing}
      data-error={Boolean(error)}
      onMouseDown={(event) => {
        if (preview || event.button !== 0 || (event.target as Element).closest('button')) return;
        event.preventDefault();
        // Native dragging can consume click events; recognize the second press here.
        const action = event.detail === 2 ? showMainWindow() : getCurrentWindow().startDragging();
        void action.catch(() => {});
      }}
    >
      <VoiceAppIcon
        key={preview ? 'preview' : 'desktop'}
        src={(!preview && snapshot.appIcon) || '/brand/bird.png'}
        loading={!preview && snapshot.appIconLoading}
      />
      <div className="voice-island-middle">
        {speaking ? (
          <VoiceFlow
            active={flowing}
            palette={palette}
            speaking={speaking}
            level={speaking ? snapshot.level : 0}
            theme={light ? 'light' : 'dark'}
          />
        ) : (
          <div className="voice-island-bars" aria-hidden="true">
            {Array.from({ length: 28 }, (_, index) => (
              <i
                key={index}
                style={{
                  height: 4 + level * (4 + Math.abs(Math.sin(index * 0.7)) * 8) + 'px',
                  animationDelay: `${(index % 7) * -0.13}s`,
                  backgroundColor: voicePaletteColor(palette, index / 27, light),
                }}
              />
            ))}
          </div>
        )}
        <span className="sr-only" role="status">
          {t((error || 'voice.phase.' + phase) as TKey)}
        </span>
      </div>
      <button
        type="button"
        className="voice-island-control voice-island-confirm"
        aria-label={t(recording ? 'voice.stop' : 'voice.speak')}
        aria-hidden={preview || undefined}
        aria-busy={waiting || undefined}
        disabled={
          preview ||
          waiting ||
          state.saving ||
          snapshot.selectionOpen ||
          (!recording && (!ready || !config.shortcutEnabled))
        }
        onClick={() => void (recording ? state.stop() : state.record())}
      >
        {waiting ? (
          <LoaderCircle size={16} className="motion-safe:animate-spin" />
        ) : recording ? (
          <Square size={13} fill="currentColor" />
        ) : (
          <Mic size={16} />
        )}
      </button>
    </div>
  );
}

export function VoiceIsland() {
  const snapshot = useVoiceInputStore((state) => state.snapshot);
  useEffect(() => {
    let stopped = false;
    const unlisteners: (() => void)[] = [];
    const keep = (stop: () => void) => {
      if (stopped) stop();
      else unlisteners.push(stop);
    };
    listen<VoiceSnapshot>('voice-input-state', (event) =>
      useVoiceInputStore.getState().accept(event.payload)
    )
      .then(async (stop) => {
        keep(stop);
        if (stopped) return;
        useVoiceInputStore.getState().accept(await voiceIslandReady());
      })
      .catch(() => {});
    listen<ThemeSelection>(THEME_CHANGED_EVENT, ({ payload }) =>
      useThemeStore.getState().sync(payload)
    )
      .then((stop) => {
        keep(stop);
        if (!stopped) void emit(THEME_REQUEST_EVENT, {}).catch(() => {});
      })
      .catch(() => {});
    return () => {
      stopped = true;
      unlisteners.forEach((stop) => stop());
    };
  }, []);
  return (
    <div className="voice-island-root">
      {snapshot && <IslandTranscript snapshot={snapshot} />}
      {snapshot && <IslandContent snapshot={snapshot} />}
    </div>
  );
}

/** Live transcript above the floating capsule; only while speech is being recognized. */
export function IslandTranscript({ snapshot }: { snapshot: VoiceSnapshot }) {
  const live = snapshot.phase === 'recording' || snapshot.phase === 'transcribing';
  if (!live || !snapshot.text.trim()) return null;
  return (
    <div className="voice-island-transcript" aria-live="polite">
      <p>{snapshot.text}</p>
    </div>
  );
}
