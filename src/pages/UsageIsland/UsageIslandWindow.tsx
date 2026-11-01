import { useCallback, useEffect, useRef, useState, type PointerEvent } from 'react';
import { emit, emitTo, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  USAGE_ISLAND_PICK_EVENT,
  USAGE_ISLAND_REQUEST_EVENT,
  USAGE_ISLAND_STATE_EVENT,
  resizeUsageIsland,
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
  items: UsageIslandItem[];
}

const COLLAPSED_SIZE = 60;
// Outlasts the width transition, so the island is never clipped while closing.
const SHRINK_DELAY = 440;
const DRAG_DISTANCE = 4;
// The window follows the content; only a screen-sized limit applies here.
const MAX_CONTENT_WIDTH = 1600;

export function UsageIslandWindow() {
  const [state, setState] = useState<UsageIslandPayload | null>(null);
  const scale = useUiScaleStore((store) => store.scale);
  const shrinkTimer = useRef<ReturnType<typeof setTimeout>>();
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
      clearTimeout(shrinkTimer.current);
    };
  }, []);
  useEffect(() => {
    if (ready) void getCurrentWindow().show();
  }, [ready]);
  const onLayout = useCallback(
    ({ expanded, width, height, centered }: IslandLayout) => {
      clearTimeout(shrinkTimer.current);
      const resize = (nextWidth: number, nextHeight: number) =>
        void resizeUsageIsland(nextWidth * scale, nextHeight * scale, centered).catch(() => {});
      if (expanded) resize(width, height);
      else
        shrinkTimer.current = setTimeout(
          () => resize(COLLAPSED_SIZE, COLLAPSED_SIZE),
          SHRINK_DELAY
        );
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
          available={MAX_CONTENT_WIDTH}
          onLayout={onLayout}
        />
      )}
    </div>
  );
}
