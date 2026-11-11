import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import {
  getUsageIslandTooltip,
  readyUsageIslandTooltip,
  USAGE_ISLAND_TOOLTIP_EVENT,
} from '../../api/usageIsland';
import {
  UsageTooltipCard,
  USAGE_TOOLTIP_PADDING,
  type UsageTooltipSnapshot,
} from './UsageIslandTooltip';
import './UsageIsland.css';

/** A passive native surface: the island owns all input, data and selection. */
export function UsageIslandTooltipWindow() {
  const [snapshot, setSnapshot] = useState<UsageTooltipSnapshot | null>(null);
  const revision = useRef(0);
  useEffect(() => {
    let stopped = false;
    let changed = false;
    const accept = (next: UsageTooltipSnapshot | null) => {
      if (stopped || (next && next.revision < revision.current)) return;
      if (next) revision.current = next.revision;
      setSnapshot(next);
    };
    const subscription = listen<UsageTooltipSnapshot>(USAGE_ISLAND_TOOLTIP_EVENT, ({ payload }) => {
      changed = true;
      accept(payload);
    }).then((unlisten) => {
      if (stopped) {
        unlisten();
        return () => {};
      }
      void getUsageIslandTooltip()
        .then((state) => {
          if (!changed) accept(state);
        })
        .catch(() => {});
      return unlisten;
    });
    return () => {
      stopped = true;
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);
  useEffect(() => {
    if (!snapshot) return;
    let stopped = false;
    void getCurrentWebview()
      .setZoom(snapshot.request.zoom)
      .then(() => {
        if (!stopped) return readyUsageIslandTooltip(snapshot.revision);
      })
      .catch(() => {});
    return () => {
      stopped = true;
    };
  }, [snapshot]);
  return (
    snapshot && (
      <div className="usage-island-tip-native" style={{ padding: USAGE_TOOLTIP_PADDING }}>
        <UsageTooltipCard {...snapshot} />
      </div>
    )
  );
}
