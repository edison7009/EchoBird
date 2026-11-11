import { useEffect, useRef, useState } from 'react';
import type { UsageIslandStyle } from '../stores/usageIslandStore';
import { UsageIslandView } from '../pages/UsageIsland/UsageIslandView';
import { usageIslandDemoItems } from '../pages/UsageIsland/usageIslandData';
import '../pages/UsageIsland/UsageIsland.css';
import './usage-preview.css';
import { usageCycleAt } from './usageCycle';

// Public illustrative sources; no account directories, native configuration or API calls.
export function DemoUsageIsland() {
  const [started] = useState(() => Math.floor(Date.now() / 1000));
  const [elapsed, setElapsed] = useState(0);
  const [active, setActive] = useState(window.parent === window);
  const [visible, setVisible] = useState(!document.hidden);
  const [reduced, setReduced] = useState(
    () => window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
  const clock = useRef(0);

  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (
        event.source === window.parent &&
        event.origin === window.location.origin &&
        event.data?.type === 'echobird-usage-preview'
      )
        setActive(event.data.visible === true);
    };
    const onVisibility = () => setVisible(!document.hidden);
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)');
    const onPreference = () => setReduced(preference.matches);
    window.addEventListener('message', onMessage);
    document.addEventListener('visibilitychange', onVisibility);
    preference.addEventListener('change', onPreference);
    window.parent.postMessage({ type: 'echobird-usage-ready' }, window.location.origin);
    return () => {
      window.removeEventListener('message', onMessage);
      document.removeEventListener('visibilitychange', onVisibility);
      preference.removeEventListener('change', onPreference);
    };
  }, []);

  useEffect(() => {
    if (!active || !visible || reduced) return;
    let last = performance.now();
    const timer = window.setInterval(() => {
      const now = performance.now();
      clock.current += now - last;
      last = now;
      setElapsed(clock.current);
    }, 100);
    return () => window.clearInterval(timer);
  }, [active, visible, reduced]);
  const cycle = reduced
    ? ({ style: 'line', expanded: true, fading: false } as const)
    : usageCycleAt(elapsed);
  const island = (style: UsageIslandStyle) => (
    <UsageIslandView
      items={usageIslandDemoItems(style, started)
        .slice(0, style === 'dial' ? 6 : 2)
        .map((item, index) => ({ ...item, smartActive: index === 0 }))}
      settings={{
        style,
        reversed: [],
        theme: 'dark',
        expandMode: 'hover',
        showSmartAccess: true,
        background: 'basalt',
      }}
      hover={cycle.expanded}
    />
  );
  return (
    <div className="website-usage-demo" data-playing={cycle.style}>
      <div key={cycle.style} className="usage-demo-scene" data-fading={cycle.fading}>
        <div className="usage-island-stage">
          <div
            className="usage-island-preview"
            aria-hidden="true"
            ref={(node) => node?.setAttribute('inert', '')}
          >
            {island(cycle.style)}
          </div>
        </div>
      </div>
    </div>
  );
}
