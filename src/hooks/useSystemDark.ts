import { useMemo, useSyncExternalStore } from 'react';

export function useSystemDark() {
  const appearance = useMemo(() => {
    const query = window.matchMedia('(prefers-color-scheme: dark)');
    return {
      getSnapshot: () => query.matches,
      subscribe: (notify: () => void) => {
        query.addEventListener('change', notify);
        return () => query.removeEventListener('change', notify);
      },
    };
  }, []);
  return useSyncExternalStore(appearance.subscribe, appearance.getSnapshot);
}
