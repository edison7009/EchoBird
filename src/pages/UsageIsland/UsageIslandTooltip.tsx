import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { listen } from '@tauri-apps/api/event';
import { cursorPosition, getCurrentWindow } from '@tauri-apps/api/window';
import { createPortal } from 'react-dom';
import { Box } from 'lucide-react';
import { QuotaCountdown } from '../AppManager/QuotaCountdown';
import { useUsageClock } from '../../hooks/useUsageClock';
import { quotaTone } from '../../utils/quotaColors';
import type { UsageIslandItem } from './usageIslandData';

export const USAGE_TOOLTIP_WIDTH = 276;
export const USAGE_TOOLTIP_PADDING = 12;
export interface UsageTooltipRequest {
  item: UsageIslandItem;
  theme: 'dark' | 'light';
  resetLabel: string;
  anchor: { x: number; y: number; width: number; height: number };
  width: number;
  height: number;
  zoom: number;
  side?: boolean;
  clearance?: UsageTooltipRequest['anchor'];
  direction?: UsageTooltipSnapshot['placement'];
}
export interface UsageTooltipSnapshot {
  request: UsageTooltipRequest;
  placement: 'above' | 'below' | 'left' | 'right';
  arrowX: number;
  revision: number;
}
export function usageTooltipHeight(item: UsageIslandItem) {
  return (
    78 +
    item.lines.reduce(
      (height, line) =>
        height + ((line.percent === null ? 36 : 46) + (line.resetAt != null ? 18 : 0)),
      0
    )
  );
}
export function usageTooltipPosition(
  anchor: UsageTooltipRequest['anchor'],
  width: number,
  height: number,
  viewport: { width: number; height: number },
  clearance?: UsageTooltipRequest['anchor'],
  direction?: UsageTooltipSnapshot['placement']
) {
  if (clearance && direction) {
    const centerX = anchor.x + anchor.width / 2;
    const centerY = anchor.y + anchor.height / 2;
    const candidates = [direction, 'above', 'below', 'left', 'right'] as const;
    for (const placement of candidates) {
      let left = Math.max(12, Math.min(viewport.width - width - 12, centerX - width / 2));
      let top = Math.max(12, Math.min(viewport.height - height - 12, centerY - height / 2));
      if (placement === 'above') top = clearance.y - height - 10;
      if (placement === 'below') top = clearance.y + clearance.height + 10;
      if (placement === 'left') left = clearance.x - width - 10;
      if (placement === 'right') left = clearance.x + clearance.width + 10;
      if (
        left < 12 ||
        top < 12 ||
        left + width > viewport.width - 12 ||
        top + height > viewport.height - 12
      )
        continue;
      const side = placement === 'left' || placement === 'right';
      return {
        left,
        top,
        placement,
        arrowX: side
          ? Math.max(18, Math.min(height - 18, centerY - top))
          : Math.max(18, Math.min(width - 18, centerX - left)),
      };
    }
    return null;
  }
  const left = Math.max(
    12,
    Math.min(viewport.width - width - 12, anchor.x + anchor.width / 2 - width / 2)
  );
  const above = anchor.y - height - 10 >= 12;
  return {
    left,
    top: Math.max(
      12,
      above
        ? anchor.y - height - 10
        : Math.min(viewport.height - height - 12, anchor.y + anchor.height + 10)
    ),
    placement: above ? ('above' as const) : ('below' as const),
    arrowX: Math.max(18, Math.min(width - 18, anchor.x + anchor.width / 2 - left)),
  };
}
export function UsageTooltipCard({
  request,
  placement,
  arrowX,
}: Omit<UsageTooltipSnapshot, 'revision'>) {
  const now = useUsageClock(request.item.lines.some((line) => line.resetAt != null));
  return (
    <div
      id="usage-island-tooltip"
      role="tooltip"
      className={`usage-island-tooltip usage-island-theme-${request.theme}`}
      data-placement={placement}
      style={
        {
          width: request.width,
          height: request.height,
          '--usage-tip-arrow-x': `${arrowX}px`,
        } as CSSProperties
      }
    >
      <div className="usage-island-tip-head">
        {request.item.icon ? (
          <img src={request.item.icon} alt="" draggable={false} />
        ) : (
          <Box aria-hidden="true" />
        )}
        <div>
          <strong>{request.item.name}</strong>
          {request.item.detail && <span>{request.item.detail}</span>}
        </div>
      </div>
      {request.item.lines.map((line, index) => (
        <div
          key={index}
          className="usage-island-tip-row"
          data-tone={quotaTone(line.percent)}
          data-bare={line.percent === null || undefined}
          style={{ height: (line.percent === null ? 36 : 46) + (line.resetAt != null ? 18 : 0) }}
        >
          <div className="usage-island-tip-row-head">
            <span>{line.label}</span>
            <strong>{line.value}</strong>
          </div>
          {line.percent !== null && (
            <div className="usage-island-tip-track">
              <span style={{ width: `${line.percent}%` }} />
            </div>
          )}
          {line.resetAt != null && line.resetAt * 1000 > now && (
            <div className="usage-island-tip-reset">
              <span>{request.resetLabel}</span>
              <QuotaCountdown resetAt={line.resetAt} compact small />
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
export function UsageTooltipOverlay({ request }: { request: UsageTooltipRequest }) {
  const [position, setPosition] = useState(() =>
    usageTooltipPosition(
      request.anchor,
      request.width,
      request.height,
      {
        width: window.innerWidth,
        height: window.innerHeight,
      },
      request.clearance,
      request.direction
    )
  );
  useLayoutEffect(() => {
    const place = () =>
      setPosition(
        usageTooltipPosition(
          request.anchor,
          request.width,
          request.height,
          {
            width: window.innerWidth,
            height: window.innerHeight,
          },
          request.clearance,
          request.direction
        )
      );
    place();
    window.addEventListener('resize', place);
    return () => window.removeEventListener('resize', place);
  }, [request]);
  if (!position) return null;
  return createPortal(
    <div className="usage-island-tip-overlay" style={{ left: position.left, top: position.top }}>
      <UsageTooltipCard request={request} placement={position.placement} arrowX={position.arrowX} />
    </div>,
    document.body
  );
}
interface Target {
  key: string;
  element: HTMLElement;
  collapsed: boolean;
}
export function useUsageTooltip({
  items,
  style,
  expanded,
  nativeHover,
  theme,
  resetLabel,
  onChange,
  resetKey,
  zoom = 1,
}: {
  items: UsageIslandItem[];
  style: string;
  expanded: boolean;
  nativeHover?: boolean | null;
  theme: 'dark' | 'light';
  resetLabel: string;
  onChange?: (request: UsageTooltipRequest | null) => void;
  resetKey?: number;
  zoom?: number;
}) {
  const [hovered, setHovered] = useState<Target | null>(null);
  const [focused, setFocused] = useState<Target | null>(null);
  const [request, setRequest] = useState<UsageTooltipRequest | null>(null);
  const notify = useRef(onChange);
  const shown = useRef(false);
  const nativeTracking = useRef(false);
  useLayoutEffect(() => {
    notify.current = onChange;
  }, [onChange]);
  const enabled = ['row', 'column', 'dial'].includes(style);
  const target = hovered ?? focused;
  const item = items.find((entry) => entry.key === target?.key);
  const serialized = JSON.stringify(item);
  useEffect(() => {
    if (!onChange || !enabled) return;
    let stopped = false;
    const update = (payload: [number, number] | null) => {
      if (stopped) return;
      nativeTracking.current = true;
      const element = payload
        ? document
            .elementFromPoint(payload[0] / zoom, payload[1] / zoom)
            ?.closest<HTMLElement>('[data-usage-tip-source]')
        : null;
      const key = element?.dataset.usageTipSource;
      setHovered((previous) => {
        if (!key || !element) return null;
        if (previous?.key === key && previous.element === element) return previous;
        return { key, element, collapsed: element.dataset.usageTipCollapsed === 'true' };
      });
    };
    let nativeReceived = false;
    const subscription = listen<[number, number] | null>('usage-island-pointer', ({ payload }) => {
      nativeReceived = true;
      update(payload);
    });
    let pending = false;
    const poll = async () => {
      if (pending || nativeReceived || stopped) return;
      pending = true;
      try {
        const window = getCurrentWindow();
        const [cursor, origin, factor] = await Promise.all([
          cursorPosition(),
          window.innerPosition(),
          window.scaleFactor(),
        ]);
        if (!stopped && !nativeReceived) {
          update([(cursor.x - origin.x) / factor, (cursor.y - origin.y) / factor]);
        }
      } catch {
        /* Native event samples remain available when the polling API is unavailable. */
      } finally {
        pending = false;
      }
    };
    const timer = setInterval(() => void poll(), 80);
    return () => {
      stopped = true;
      clearInterval(timer);
      void subscription.then((stop) => stop()).catch(() => {});
    };
  }, [onChange, enabled, zoom]);

  useEffect(() => {
    if (target && !item) {
      // Removed sources must not retain anchors that could reappear later.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setHovered(null);

      setFocused(null);
    }
  }, [target, item]);
  useEffect(() => {
    // A new layout invalidates anchors and focus from the previous rendering.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setHovered(null);

    setFocused(null);
  }, [style, resetKey]);
  useEffect(() => {
    if (!expanded) {
      // The logo may still describe its selected source while collapsed.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setHovered((current) => (current?.collapsed ? current : null));

      setFocused((current) => (current?.collapsed ? current : null));
    }
  }, [expanded]);
  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.addEventListener !== 'function') return;
    const dismiss = () => {
      setHovered(null);
      setFocused(null);
    };
    window.addEventListener('scroll', dismiss, true);
    window.addEventListener('resize', dismiss);
    window.addEventListener('blur', dismiss);
    return () => {
      window.removeEventListener('scroll', dismiss, true);
      window.removeEventListener('resize', dismiss);
      window.removeEventListener('blur', dismiss);
    };
  }, []);
  useEffect(() => {
    if (nativeHover === false) {
      // Native hit testing can stop DOM leave events outside the island.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setHovered(null);
    }
  }, [nativeHover]);
  useLayoutEffect(() => {
    const source = serialized ? (JSON.parse(serialized) as UsageIslandItem) : undefined;
    const valid = enabled && source && target && (expanded || target.collapsed);
    const rect = valid
      ? (
          target.element.querySelector?.('.usage-island-gauge') ?? target.element
        ).getBoundingClientRect()
      : null;
    const disk =
      style === 'dial' && target
        ? target.element.closest?.('.usage-island')?.getBoundingClientRect()
        : null;
    let direction: UsageTooltipSnapshot['placement'] | undefined;
    if (disk && rect) {
      const x = rect.left + rect.width / 2 - (disk.left + disk.width / 2);
      const y = rect.top + rect.height / 2 - (disk.top + disk.height / 2);
      direction =
        Math.abs(y) >= Math.abs(x) ? (y < 0 ? 'above' : 'below') : x < 0 ? 'left' : 'right';
    }
    const next: UsageTooltipRequest | null =
      rect && source
        ? {
            item: source,
            theme,
            resetLabel,
            anchor: { x: rect.left, y: rect.top, width: rect.width, height: rect.height },
            width: USAGE_TOOLTIP_WIDTH,
            height: usageTooltipHeight(source),
            zoom: 1,
            side: style === 'column',
            ...(disk
              ? {
                  clearance: { x: disk.left, y: disk.top, width: disk.width, height: disk.height },
                  direction,
                }
              : {}),
          }
        : null;
    // Position before paint so a changed source never flashes the previous card.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setRequest(next);
    if (next || shown.current) notify.current?.(next);
    shown.current = next !== null;
  }, [serialized, target, enabled, expanded, theme, resetLabel, style]);
  useEffect(
    () => () => {
      if (shown.current) notify.current?.(null);
    },
    []
  );
  const dismiss = () => {
    setHovered(null);
    setFocused(null);
  };
  return {
    request: enabled && item && (expanded || target?.collapsed) ? request : null,
    dismiss,
    handlers: (key: string, collapsed = false) =>
      enabled
        ? {
            'data-usage-tip-source': key,
            'data-usage-tip-collapsed': collapsed || undefined,
            onPointerEnter: (event: { pointerType: string; currentTarget: HTMLElement }) => {
              if (event.pointerType === 'mouse')
                setHovered({ key, element: event.currentTarget, collapsed });
            },
            onPointerLeave: () => {
              if (!nativeTracking.current)
                setHovered((current) => (current?.key === key ? null : current));
            },
            onFocus: (event: { currentTarget: HTMLElement }) => {
              if (event.currentTarget.matches?.(':focus-visible') !== false)
                setFocused({ key, element: event.currentTarget, collapsed });
            },
            onBlur: () => setFocused((current) => (current?.key === key ? null : current)),
            onPointerDown: dismiss,
            onKeyDown: (event: { key: string }) => {
              if (event.key === 'Escape') dismiss();
            },
            'aria-describedby':
              !onChange && request?.item.key === key ? 'usage-island-tooltip' : undefined,
          }
        : {},
  };
}
