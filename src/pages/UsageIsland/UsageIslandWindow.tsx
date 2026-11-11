import { useCallback, useEffect, useRef, useState, type PointerEvent } from 'react';
import { emit, emitTo, listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  USAGE_ISLAND_ACTION_EVENT,
  USAGE_ISLAND_HOVER_EVENT,
  USAGE_ISLAND_PICK_EVENT,
  USAGE_ISLAND_REQUEST_EVENT,
  USAGE_ISLAND_STATE_EVENT,
  USAGE_ISLAND_TICK_EVENT,
  resizeUsageIsland,
  setUsageIslandHitArea,
  showUsageIslandTooltip,
} from '../../api/usageIsland';
import {
  THEME_CHANGED_EVENT,
  THEME_REQUEST_EVENT,
  useThemeStore,
  type ThemeSelection,
} from '../../stores/themeStore';
import type { UsageTooltipRequest } from './UsageIslandTooltip';
import type { UsageIslandItem } from './usageIslandData';
import {
  UsageIslandView,
  type IslandLayout,
  type IslandSettings,
  type UsageIslandActions,
} from './UsageIslandView';
import '../VoiceInput/VoiceInput.css';
import './UsageIsland.css';

export interface UsageIslandPayload {
  settings: IslandSettings;
  actions?: UsageIslandActions;
  pick: string | null;
  /** The island stays visible, so it keeps the refresh time for the main window. */
  refreshSeconds: number;
  scale: number;
  items: UsageIslandItem[];
}

const COLLAPSED_SIZE = 48;
// Room for the island's shadow, which the window edge would otherwise cut off.
const SHADOW_PADDING = 24;
const DRAG_DISTANCE = 4;
// The window follows the content; only a screen-sized limit applies here.
const MAX_CONTENT_WIDTH = 1600;

export function UsageIslandWindow() {
  const [state, setState] = useState<UsageIslandPayload | null>(null);
  const scale = (state?.scale ?? 100) / 100;
  const [hover, setHover] = useState<boolean | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  // The first frame must already be at the saved size.
  const zoomed = useRef(Promise.resolve());
  const ready = state !== null;
  const layoutRevision = useRef(0);
  const layoutQueue = useRef(Promise.resolve());
  const windowGeometry = useRef<string | null>(null);
  useEffect(
    () => () => {
      layoutRevision.current += 1;
    },
    []
  );
  useEffect(() => {
    let stopped = false;
    const stops: (() => void)[] = [];
    const keep = (stop: () => void) => {
      if (stopped) stop();
      else stops.push(stop);
    };
    listen<string>(USAGE_ISLAND_STATE_EVENT, ({ payload }) =>
      setState(JSON.parse(payload) as UsageIslandPayload)
    )
      .then((stop) => {
        keep(stop);
        if (!stopped) void emitTo('main', USAGE_ISLAND_REQUEST_EVENT, {}).catch(() => {});
      })
      .catch(() => {});
    listen<boolean>(USAGE_ISLAND_HOVER_EVENT, ({ payload }) => setHover(payload))
      .then(keep)
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
      stops.forEach((stop) => stop());
    };
  }, []);
  useEffect(() => {
    if (!ready) return;
    zoomed.current = getCurrentWebview()
      .setZoom(scale)
      .catch(() => {});
  }, [ready, scale]);
  useEffect(() => {
    if (ready) void zoomed.current.then(() => getCurrentWindow().show());
  }, [ready]);
  const refreshSeconds = state?.refreshSeconds ?? 0;
  useEffect(() => {
    if (!refreshSeconds) return;
    const timer = setInterval(
      () => void emitTo('main', USAGE_ISLAND_TICK_EVENT, {}).catch(() => {}),
      refreshSeconds * 1000
    );
    return () => clearInterval(timer);
  }, [refreshSeconds]);
  const onLayout = useCallback(
    ({ expanded, width, height, radius, anchorX, anchorY }: IslandLayout) => {
      const revision = ++layoutRevision.current;
      const geometry = [
        (width + SHADOW_PADDING * 2) * scale,
        (height + SHADOW_PADDING * 2) * scale,
        anchorX,
        anchorY,
      ] as const;
      const signature = JSON.stringify(geometry);
      const area = expanded
        ? [SHADOW_PADDING, SHADOW_PADDING, width, height]
        : [
            SHADOW_PADDING + (width - COLLAPSED_SIZE) * anchorX,
            SHADOW_PADDING + (height - COLLAPSED_SIZE) * anchorY,
            COLLAPSED_SIZE,
            COLLAPSED_SIZE,
          ];
      const [x, y, areaWidth, areaHeight] = area.map((value) => value * scale);
      const hitRadius =
        (expanded ? (radius ?? Math.min(width, height) / 2) : COLLAPSED_SIZE / 2) * scale;
      // Keep native resize/hit updates in order and discard obsolete transition frames.
      // Open/close only changes the hit area; the native window keeps its full size.
      layoutQueue.current = layoutQueue.current
        .catch(() => {})
        .then(async () => {
          if (revision !== layoutRevision.current) return;
          await zoomed.current;
          if (revision !== layoutRevision.current) return;
          if (signature !== windowGeometry.current) {
            await resizeUsageIsland(...geometry);
            windowGeometry.current = signature;
          }
          if (revision !== layoutRevision.current) return;
          await setUsageIslandHitArea(x, y, areaWidth, areaHeight, hitRadius);
        })
        .catch(() => {});
    },
    [scale]
  );
  const onTooltipChange = useCallback(
    (tooltip: UsageTooltipRequest | null) => {
      void showUsageIslandTooltip(tooltip ? { ...tooltip, zoom: scale } : null).catch(() => {});
    },
    [scale]
  );
  const onPick = useCallback((key: string | null) => {
    void emitTo('main', USAGE_ISLAND_PICK_EVENT, key).catch(() => {});
  }, []);
  const onPointerMove = (event: PointerEvent) => {
    const start = press.current;
    if (!start) return;
    // The release can land outside once the window shrinks; never drag without a press.
    if (!(event.buttons & 1)) {
      press.current = null;
      return;
    }
    if (Math.hypot(event.clientX - start.x, event.clientY - start.y) < DRAG_DISTANCE) return;
    press.current = null;
    void getCurrentWindow()
      .startDragging()
      .catch(() => {});
  };
  return (
    <div
      className="usage-island-window-root"
      onPointerDown={(event) => {
        void showUsageIslandTooltip(null).catch(() => {});
        if (event.button === 0) press.current = { x: event.clientX, y: event.clientY };
      }}
      onPointerMove={onPointerMove}
      onPointerUp={() => {
        press.current = null;
      }}
    >
      {state && (
        <UsageIslandView
          items={state.items}
          settings={state.settings}
          pick={state.pick}
          onPick={onPick}
          actions={state.actions}
          onAction={(action) => {
            void emitTo('main', USAGE_ISLAND_ACTION_EVENT, action).catch(() => {});
          }}
          hover={hover}
          masked
          available={Math.min(MAX_CONTENT_WIDTH, window.screen.availWidth / scale)}
          onLayout={onLayout}
          onTooltipChange={onTooltipChange}
          tooltipResetKey={scale}
        />
      )}
    </div>
  );
}
