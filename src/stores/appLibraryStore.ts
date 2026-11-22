import { create } from 'zustand';
import {
  appLibrary,
  appLibraryOwner,
  type LibraryEntry,
  type LibraryKind,
} from '../api/appLibrary';

interface LibrarySnapshot {
  entries?: LibraryEntry[];
  error?: unknown;
}

interface AppLibraryState {
  snapshots: Partial<Record<string, Partial<Record<LibraryKind, LibrarySnapshot>>>>;
  read: (toolId: string, kind: LibraryKind, force?: boolean) => Promise<void>;
}

const pending = new Map<string, Promise<void>>();

export const useAppLibraryStore = create<AppLibraryState>((set) => ({
  snapshots: {},
  read: (toolId, kind, force = false) => {
    const owner = appLibraryOwner(toolId, kind);
    const key = `${owner}/${kind}`;
    const current = pending.get(key);
    if (current && !force) return current;

    const publish = (snapshot: LibrarySnapshot) => {
      if (pending.get(key) !== request) return;
      set(({ snapshots }) => ({
        snapshots: {
          ...snapshots,
          [owner]: {
            ...snapshots[owner],
            [kind]: snapshot.entries ? snapshot : { ...snapshots[owner]?.[kind], ...snapshot },
          },
        },
      }));
    };
    const request: Promise<void> = Promise.resolve()
      .then(() => appLibrary.list(toolId, kind))
      .then(
        (entries) => publish({ entries }),
        (error: unknown) => publish({ error })
      )
      .finally(() => {
        if (pending.get(key) === request) pending.delete(key);
      });
    pending.set(key, request);
    return request;
  },
}));
