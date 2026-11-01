import { useEffect, useRef } from 'react';
import { emitTo, listen } from '@tauri-apps/api/event';
import {
  USAGE_ISLAND_PICK_EVENT,
  USAGE_ISLAND_REQUEST_EVENT,
  USAGE_ISLAND_STATE_EVENT,
  USAGE_ISLAND_TICK_EVENT,
  USAGE_ISLAND_WINDOW,
} from '../../api/usageIsland';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { useUsageIslandStore } from '../../stores/usageIslandStore';
import type { UsageIslandPayload } from './UsageIslandWindow';
import { useUsageIslandItems, useUsageIslandRefresh, useUsageSources } from './useUsageIslandData';

/** Keeps the floating window in step; only the main window owns accounts and models. */
export default function UsageIslandBridge() {
  const style = useUsageIslandStore((state) => state.style);
  const reversed = useUsageIslandStore((state) => state.reversed);
  const theme = useUsageIslandStore((state) => state.theme);
  const expandMode = useUsageIslandStore((state) => state.expandMode);
  const pick = useUsageIslandStore((state) => state.pick);
  const refreshSeconds = useUsageIslandStore((state) => state.refreshSeconds);
  const hidden = useAccountPrivacyStore((privacy) => privacy.hidden);
  const items = useUsageIslandItems(useUsageSources(true));
  // The browser slows timers in a window hidden to the tray, so the island keeps time.
  const { refresh } = useUsageIslandRefresh(items, true, true, true);
  const latestRefresh = useRef(refresh);
  useEffect(() => {
    latestRefresh.current = refresh;
  });
  const payload: UsageIslandPayload = {
    settings: { style, reversed, theme, expandMode },
    pick,
    refreshSeconds,
    items: items.map(({ kind, key, name, detail, icon, percent, value, lines }) => ({
      kind,
      key,
      name,
      detail: kind === 'account' && hidden ? ACCOUNT_NAME_MASK : detail,
      icon,
      percent,
      value,
      lines,
    })),
  };
  const serialized = JSON.stringify(payload);
  const latest = useRef(serialized);
  useEffect(() => {
    latest.current = serialized;
    void emitTo(USAGE_ISLAND_WINDOW, USAGE_ISLAND_STATE_EVENT, serialized).catch(() => {});
  }, [serialized]);
  useEffect(() => {
    const stops = [
      listen(USAGE_ISLAND_REQUEST_EVENT, () => {
        void emitTo(USAGE_ISLAND_WINDOW, USAGE_ISLAND_STATE_EVENT, latest.current).catch(() => {});
      }),
      listen<string | null>(USAGE_ISLAND_PICK_EVENT, ({ payload: key }) => {
        useUsageIslandStore.getState().update({ pick: key });
      }),
      listen(USAGE_ISLAND_TICK_EVENT, () => void latestRefresh.current(false)),
    ];
    return () => stops.forEach((stop) => void stop.then((unlisten) => unlisten()).catch(() => {}));
  }, []);
  return null;
}
