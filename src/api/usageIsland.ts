import { invoke } from '@tauri-apps/api/core';

export const USAGE_ISLAND_WINDOW = 'usage-island';
export const USAGE_ISLAND_STATE_EVENT = 'usage-island-state';
export const USAGE_ISLAND_REQUEST_EVENT = 'usage-island-request';
export const USAGE_ISLAND_PICK_EVENT = 'usage-island-pick';

export const setUsageIslandVisible = (visible: boolean) =>
  invoke<void>('usage_island_set_visible', { visible });
export const resizeUsageIsland = (width: number, height: number, centered: boolean) =>
  invoke<void>('usage_island_resize', { width, height, centered });
