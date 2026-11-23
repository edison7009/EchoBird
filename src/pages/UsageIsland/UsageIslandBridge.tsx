import { useEffect, useRef } from 'react';
import { emitTo, listen } from '@tauri-apps/api/event';
import {
  USAGE_ISLAND_ACTION_EVENT,
  USAGE_ISLAND_PICK_EVENT,
  USAGE_ISLAND_REQUEST_EVENT,
  USAGE_ISLAND_STATE_EVENT,
  USAGE_ISLAND_TICK_EVENT,
  USAGE_ISLAND_WINDOW,
  updateTrayUsage,
} from '../../api/usageIsland';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { useBackgroundStore } from '../../stores/backgroundStore';
import { useUsageIslandStore } from '../../stores/usageIslandStore';
import type { UsageIslandAction } from './usageIslandData';
import type { UsageIslandPayload } from './UsageIslandWindow';
import {
  useUsageIslandItems,
  useUsageIslandRefresh,
  useUsageIslandSmartActivity,
  useUsageSources,
} from './useUsageIslandData';

/** Keeps the floating window in step; only the main window owns accounts and models. */
export default function UsageIslandBridge({
  floating,
  tray,
}: {
  floating: boolean;
  tray: boolean;
}) {
  const style = useUsageIslandStore((state) => state.style);
  const reversed = useUsageIslandStore((state) => state.reversed);
  const theme = useUsageIslandStore((state) => state.theme);
  const expandMode = useUsageIslandStore((state) => state.expandMode);
  const pick = useUsageIslandStore((state) => state.pick);
  const refreshSeconds = useUsageIslandStore((state) => state.refreshSeconds);
  const scale = useUsageIslandStore((state) => state.scale);
  const opacity = useUsageIslandStore((state) => state.opacity);
  const showSmartAccess = useUsageIslandStore((state) => state.showSmartAccess);
  const background = useBackgroundStore((state) => state.mode);
  const hidden = useAccountPrivacyStore((privacy) => privacy.hidden);
  const hasSources = useUsageIslandStore((state) => state.sources.length > 0);
  const items = useUsageIslandItems(useUsageSources(hasSources));
  const smartActiveKeys = useUsageIslandSmartActivity(showSmartAccess && items.length > 0, items);
  const hosted = floating || tray;
  // A visible floating window keeps time even when the main window is hidden.
  const { refresh, actions, run } = useUsageIslandRefresh(
    items,
    floating ? 'floating' : tray ? 'tray' : null
  );
  const latestRefresh = useRef(refresh);
  const latestRun = useRef(run);
  useEffect(() => {
    latestRefresh.current = refresh;
    latestRun.current = run;
  });
  const payload: UsageIslandPayload = {
    settings: { style, reversed, theme, expandMode, opacity, showSmartAccess, background },
    pick,
    refreshSeconds,
    scale,
    actions,
    items: items.map(({ kind, key, name, detail, icon, percent, value, lines }) => ({
      kind,
      key,
      name,
      detail: kind === 'account' && hidden ? ACCOUNT_NAME_MASK : detail,
      icon,
      percent,
      value,
      lines,
      smartActive: smartActiveKeys.has(key),
    })),
  };
  const serialized = JSON.stringify(payload);
  const latest = useRef(serialized);
  useEffect(() => {
    latest.current = serialized;
    if (floating)
      void emitTo(USAGE_ISLAND_WINDOW, USAGE_ISLAND_STATE_EVENT, serialized).catch(() => {});
  }, [floating, serialized]);

  // No usage requests on hover. The tray receives only what is already visible in the island.
  const trayPreview = tray
    ? JSON.stringify({
        theme,
        background,
        opacity,
        scale,
        refreshSeconds: floating ? 0 : refreshSeconds,
        items: payload.items,
      })
    : null;
  useEffect(() => {
    if (trayPreview) void updateTrayUsage(JSON.parse(trayPreview)).catch(() => {});
  }, [trayPreview]);
  useEffect(() => {
    if (tray) return () => void updateTrayUsage(null).catch(() => {});
  }, [tray]);
  useEffect(() => {
    if (!hosted) return;
    const stop = listen(USAGE_ISLAND_TICK_EVENT, () => {
      if (useUsageIslandStore.getState().refreshSeconds) void latestRefresh.current(false);
    });
    return () => void stop.then((unlisten) => unlisten()).catch(() => {});
  }, [hosted]);
  useEffect(() => {
    if (!floating) return;
    const stops = [
      listen(USAGE_ISLAND_REQUEST_EVENT, () => {
        void emitTo(USAGE_ISLAND_WINDOW, USAGE_ISLAND_STATE_EVENT, latest.current).catch(() => {});
      }),
      listen<string | null>(USAGE_ISLAND_PICK_EVENT, ({ payload: key }) => {
        useUsageIslandStore.getState().update({ pick: key });
      }),
      listen<UsageIslandAction>(USAGE_ISLAND_ACTION_EVENT, ({ payload: action }) => {
        if (
          useUsageIslandStore.getState().style !== 'list' ||
          !['warmUp', 'claim', 'refresh'].includes(action)
        )
          return;
        void latestRun.current(action);
      }),
    ];
    return () => stops.forEach((stop) => void stop.then((unlisten) => unlisten()).catch(() => {}));
  }, [floating]);
  return null;
}
