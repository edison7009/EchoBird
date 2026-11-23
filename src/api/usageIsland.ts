import { invoke } from '@tauri-apps/api/core';
import type { BackgroundMode } from '../stores/backgroundStore';
import type { UsageIslandOpacity, UsageIslandTheme } from '../stores/usageIslandStore';
import type { UsageIslandItem } from '../pages/UsageIsland/usageIslandData';
import type {
  UsageTooltipRequest,
  UsageTooltipSnapshot,
} from '../pages/UsageIsland/UsageIslandTooltip';

export const USAGE_ISLAND_WINDOW = 'usage-island';
export const USAGE_ISLAND_STATE_EVENT = 'usage-island-state';
export const USAGE_ISLAND_REQUEST_EVENT = 'usage-island-request';
export const USAGE_ISLAND_PICK_EVENT = 'usage-island-pick';
export const USAGE_ISLAND_HOVER_EVENT = 'usage-island-hover';
export const USAGE_ISLAND_TICK_EVENT = 'usage-island-tick';
export const USAGE_ISLAND_ACTION_EVENT = 'usage-island-action';
export const USAGE_ISLAND_TOOLTIP_EVENT = 'usage-island-tooltip-state';

export const showUsageIslandTooltip = (tooltip: UsageTooltipRequest | null) =>
  invoke<void>('usage_island_tooltip', { tooltip });
export const getUsageIslandTooltip = () =>
  invoke<UsageTooltipSnapshot | null>('usage_island_tooltip_state');
export const readyUsageIslandTooltip = (revision: number) =>
  invoke<void>('usage_island_tooltip_ready', { revision });

export const setUsageIslandVisible = (visible: boolean) =>
  invoke<void>('usage_island_set_visible', { visible });
export const resizeUsageIsland = (
  width: number,
  height: number,
  anchorX: number,
  anchorY: number
) => invoke<void>('usage_island_resize', { width, height, anchorX, anchorY });
export const setUsageIslandHitArea = (
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number
) => invoke<void>('usage_island_set_hit_area', { x, y, width, height, radius });

export const TRAY_USAGE_WINDOW = 'tray-usage';
export const TRAY_USAGE_STATE_EVENT = 'tray-usage-state';

export interface TrayUsagePreview {
  theme: UsageIslandTheme;
  background?: BackgroundMode;
  opacity: UsageIslandOpacity;
  scale: number;
  refreshSeconds: number;
  items: UsageIslandItem[];
}

export interface TrayUsageSnapshot {
  revision: number;
  preview: TrayUsagePreview | null;
}

// The main window owns the preview. Keep updates and the final clear ordered.
let trayUsageQueue = Promise.resolve();
export function updateTrayUsage(preview: TrayUsagePreview | null) {
  trayUsageQueue = trayUsageQueue
    .catch(() => {})
    .then(() => invoke<void>('tray_usage_update', { preview }));
  return trayUsageQueue;
}

export const getTrayUsage = () => invoke<TrayUsageSnapshot | null>('tray_usage_state');
export const readyTrayUsage = (revision: number) => invoke<void>('tray_usage_ready', { revision });
