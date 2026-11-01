import { invoke } from '@tauri-apps/api/core';

export const USAGE_ISLAND_WINDOW = 'usage-island';
export const USAGE_ISLAND_STATE_EVENT = 'usage-island-state';
export const USAGE_ISLAND_REQUEST_EVENT = 'usage-island-request';
export const USAGE_ISLAND_PICK_EVENT = 'usage-island-pick';
export const USAGE_ISLAND_HOVER_EVENT = 'usage-island-hover';

export const setUsageIslandVisible = (visible: boolean) =>
  invoke<void>('usage_island_set_visible', { visible });
export const resizeUsageIsland = (
  width: number,
  height: number,
  anchorX: number,
  anchorY: number
) => invoke<void>('usage_island_resize', { width, height, anchorX, anchorY });
export const setUsageIslandHitArea = (x: number, y: number, width: number, height: number) =>
  invoke<void>('usage_island_set_hit_area', { x, y, width, height });
