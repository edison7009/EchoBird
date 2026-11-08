import type { AppSettings } from '../api/types';
import { useUsageIslandStore } from './usageIslandStore';
import { useNavigationStore } from './navigationStore';

const MIGRATED = 'echobird-merged-pages-v1';

export function initializeMergedPageFeatures(
  settings: Pick<AppSettings, 'developerMode'> | null,
  islandAtRead = useUsageIslandStore.getState()
) {
  let stop: (() => void) | undefined;
  const rememberExplicitChoice = () => {
    try {
      localStorage.setItem(MIGRATED, '1');
    } catch {
      // The explicit preference still applies in this session.
    }
  };
  if (settings) {
    migrateMergedPages(settings, islandAtRead);
  } else if (useUsageIslandStore.getState() === islandAtRead) {
    // Failed IPC must not leave pages blank or start a previously paused window.
    // Keep the saved preference intact so an explicit action can retry it later.
    useUsageIslandStore.setState({ enabled: false });
    stop = useUsageIslandStore.subscribe((next, previous) => {
      if (next.enabled === previous.enabled) return;
      rememberExplicitChoice();
      stop?.();
    });
  } else {
    rememberExplicitChoice();
  }
  useNavigationStore.getState().setDeveloperMode(true);
  return stop;
}

// Preserve an island that was effectively off under the retired global switch.
// Visiting a merged page must never turn a floating window on.
export function migrateMergedPages(
  settings: Pick<AppSettings, 'developerMode'>,
  islandAtRead = useUsageIslandStore.getState()
) {
  try {
    if (localStorage.getItem(MIGRATED)) return;
    if (
      settings.developerMode === false &&
      useUsageIslandStore.getState() === islandAtRead &&
      islandAtRead.enabled
    ) {
      useUsageIslandStore.getState().update({ enabled: false });
    }
    localStorage.setItem(MIGRATED, '1');
  } catch {
    // Preserve the old effective-off state for this session if migration storage is unavailable.
    if (settings.developerMode === false && useUsageIslandStore.getState() === islandAtRead) {
      useUsageIslandStore.setState({ enabled: false });
    }
  }
}
