import { useCallback, useEffect, useRef, useState, type PointerEvent } from 'react';
import { emit, emitTo, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  USAGE_ISLAND_HOVER_EVENT,
  USAGE_ISLAND_PICK_EVENT,
  USAGE_ISLAND_REQUEST_EVENT,
  USAGE_ISLAND_STATE_EVENT,
  USAGE_ISLAND_TICK_EVENT,
  resizeUsageIsland,
  setUsageIslandHitArea,
} from '../../api/usageIsland';
import {
  THEME_CHANGED_EVENT,
  THEME_REQUEST_EVENT,
  useThemeStore,
  type ThemeSelection,
} from '../../stores/themeStore';
import { useUiScaleStore } from '../../stores/uiScaleStore';
import type { UsageIslandItem } from './usageIslandData';
import { UsageIslandView, type IslandLayout, type IslandSettings } from './UsageIslandView';
import '../VoiceInput/VoiceInput.css';
import './UsageIsland.css';

export interface UsageIslandPayload {
  settings: IslandSettings;
  pick: string | null;
  /** The island stays visible, so it keeps the refresh time for the main window. */
  refreshSeconds: number;
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
  const scale = useUiScaleStore((store) => store.scale);
  const [hover, setHover] = useState<boolean | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  const ready = state !== null;
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
    if (ready) void getCurrentWindow().show();
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
    ({ expanded, width, height, anchorX, anchorY }: IslandLayout) => {
      // Moving a window on hover flashes its old frame, so it keeps its open size.
      void resizeUsageIsland(
        (width + SHADOW_PADDING * 2) * scale,
        (height + SHADOW_PADDING * 2) * scale,
        anchorX,
        anchorY
      ).catch(() => {});
      const area = expanded
        ? [SHADOW_PADDING, SHADOW_PADDING, width, height]
        : [
            SHADOW_PADDING + (width - COLLAPSED_SIZE) * anchorX,
            SHADOW_PADDING + (height - COLLAPSED_SIZE) * anchorY,
            COLLAPSED_SIZE,
            COLLAPSED_SIZE,
          ];
      const [x, y, areaWidth, areaHeight] = area.map((value) => value * scale);
      void setUsageIslandHitArea(x, y, areaWidth, areaHeight).catch(() => {});
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
          hover={hover}
          masked
          available={Math.min(MAX_CONTENT_WIDTH, window.screen.availWidth / scale)}
          onLayout={onLayout}
        />
      )}
    </div>
  );
}
