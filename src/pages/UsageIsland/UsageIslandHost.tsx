import { Suspense, lazy, useEffect } from 'react';
import { setUsageIslandVisible } from '../../api/usageIsland';
import { useUsageIslandVisible } from '../../stores/usageIslandStore';

const UsageIslandBridge = lazy(() => import('./UsageIslandBridge'));

// Commands run concurrently, so a quick off must wait for the window an on is building.
let windowChange = Promise.resolve();

export function UsageIslandHost() {
  const visible = useUsageIslandVisible();
  useEffect(() => {
    windowChange = windowChange
      .then(() => setUsageIslandVisible(visible))
      .catch((error) => {
        console.error('[UsageIsland] Could not change the island window:', error);
      });
  }, [visible]);
  return visible ? (
    <Suspense fallback={null}>
      <UsageIslandBridge />
    </Suspense>
  ) : null;
}
