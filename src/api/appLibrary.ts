import { invoke } from '@tauri-apps/api/core';

export type LibraryKind = 'skills' | 'mcp' | 'plugins' | 'memory';

export interface LibraryEntry {
  id: string;
  name: string;
  description: string | null;
  group: string | null;
  path: string | null;
  enabled: boolean | null;
  canCopy: boolean;
  canRemove: boolean;
  canToggle: boolean;
}

export interface LibraryCopyResult {
  toolId: string;
  status: string;
}

export function appLibraryOwner(toolId: string, kind?: LibraryKind): string {
  switch (toolId) {
    case 'claudedesktop':
      return kind === 'memory' ? 'claudecode' : toolId;
    case 'antigravity':
      return kind && kind !== 'skills' ? 'antigravitydesktop' : toolId;
    case 'chatgptdesktop':
      return 'codex';
    case 'kimidesktop':
      return 'kimicode';
    default:
      return toolId;
  }
}

export const appLibrary = {
  list: (toolId: string, kind: LibraryKind) =>
    invoke<LibraryEntry[]>('list_app_library', { toolId, kind }),
  copy: (toolId: string, kind: LibraryKind, id: string, targets: string[]) =>
    invoke<LibraryCopyResult[]>('copy_app_library_entry', { toolId, kind, id, targets }),
  remove: (toolId: string, kind: LibraryKind, id: string) =>
    invoke<void>('remove_app_library_entry', { toolId, kind, id }),
  toggle: (toolId: string, kind: LibraryKind, id: string, enabled: boolean) =>
    invoke<void>('toggle_app_library_entry', { toolId, kind, id, enabled }),
  addMcp: (toolId: string, name: string, definition: string) =>
    invoke<void>('add_app_library_mcp', { toolId, name, definition }),
};
