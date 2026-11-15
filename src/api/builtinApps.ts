import { invoke } from '@tauri-apps/api/core';
import catalog from '../data/bundledApps.json';

export const isBundledApp = (id: string) =>
  catalog.apps.some((app) => app.id === id && app.bundled);
export const acquireBuiltinApp = (id: string) => invoke<void>('acquire_builtin_app', { id });
