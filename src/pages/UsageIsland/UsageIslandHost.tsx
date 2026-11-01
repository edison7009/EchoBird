import { Suspense, lazy, useEffect } from 'react';
import { setUsageIslandVisible } from '../../api/usageIsland';
import { useUsageIslandVisible } from '../../stores/usageIslandStore';

const UsageIslandBridge = lazy(() => import('./UsageIslandBridge'));

export function UsageIslandHost() {
  const visible = useUsageIslandVisible();
  useEffect(() => {
    void setUsageIslandVisible(visible).catch((error) => {
      console.error('[UsageIsland] Could not change the island window:', error);
    });
  }, [visible]);
  return visible ? (
    <Suspense fallback={null}>
      <UsageIslandBridge />
    </Suspense>
  ) : null;
}
