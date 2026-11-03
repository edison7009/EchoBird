import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { Box } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useSystemDark } from '../../hooks/useSystemDark';
import { useThemeStore } from '../../stores/themeStore';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import type { UsageIslandConfig, UsageIslandStyle } from '../../stores/usageIslandStore';
import { quotaTone } from '../../utils/quotaColors';
import { QuotaCountdown } from '../AppManager/QuotaCountdown';
import { compactValue, type UsageIslandItem } from './usageIslandData';

const LOGO_SIZE = 48;
const COLUMN_ITEM_HEIGHT = 54;
// Logo slot plus the end padding, and the gap between sources; both mirror the CSS.
const TOOLS_INSET = 60;
const TOOLS_GAP = 12;
// Per-source widths below which a line source drops its countdown, then its second quota.
const LINE_FULL_WIDTH = 180;
const LINE_MIN_WIDTH = 118;
const DIAL_SIZE = 216;
const DIAL_CENTER = DIAL_SIZE / 2;
const DIAL_ITEM_RADIUS = 70;

interface ItemDisplay {
  label: string;
  name: string;
  tone: string;
}

export type IslandSettings = Pick<UsageIslandConfig, 'style' | 'reversed' | 'theme' | 'expandMode'>;

export interface IslandLayout {
  expanded: boolean;
  width: number;
  height: number;
  // Which part of the window stays put when it resizes: 0 start, 0.5 center, 1 end.
  anchorX: number;
  anchorY: number;
}

const islandLayout = (style: UsageIslandStyle) => (style === 'line' ? 'row' : style);

function useIslandFit(layout: string) {
  const root = useRef<HTMLDivElement>(null);
  const tools = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState({ available: 0, content: 0 });
  useLayoutEffect(() => {
    const stage = root.current;
    const content = tools.current;
    if (!stage || !content) return;
    const observer = new ResizeObserver(() =>
      setFit({
        available: stage.clientWidth,
        // Computed, because the page preview scales by a transform that layout must not include.
        content: Math.ceil(parseFloat(getComputedStyle(content).width)),
      })
    );
    observer.observe(stage);
    observer.observe(content);
    return () => observer.disconnect();
  }, [layout]);
  return { root, tools, fit };
}

function lineDensity(available: number, count: number) {
  if (!available) return undefined;
  const share = (available - TOOLS_INSET - TOOLS_GAP * (count - 1)) / count;
  if (share < LINE_MIN_WIDTH) return 'tight';
  return share < LINE_FULL_WIDTH ? 'compact' : undefined;
}

function SourceIcon({ icon }: { icon: string | null }) {
  return icon ? (
    <img src={icon} alt="" className="usage-island-icon" draggable={false} />
  ) : (
    <Box aria-hidden="true" className="usage-island-icon" />
  );
}

function Gauge({ percent, icon }: { percent: number | null; icon: string | null }) {
  return (
    <span className="usage-island-gauge">
      <svg viewBox="0 0 32 32" aria-hidden="true">
        <circle className="usage-island-gauge-track" cx="16" cy="16" r="14.5" pathLength="100" />
        {!!percent && (
          <circle
            className="usage-island-gauge-value"
            cx="16"
            cy="16"
            r="14.5"
            pathLength="100"
            strokeDasharray={`${percent} 100`}
          />
        )}
      </svg>
      <SourceIcon icon={icon} />
    </span>
  );
}

function UsageRows({ item }: { item: UsageIslandItem }) {
  return (
    <span className="usage-island-rows">
      {item.lines.map((line, index) => (
        <span key={index} className="usage-island-row" data-tone={quotaTone(line.percent)}>
          <span className="usage-island-amount">{line.value}</span>
          {line.resetAt != null ? (
            <span className="usage-island-reset">
              <QuotaCountdown resetAt={line.resetAt} compact small />
            </span>
          ) : (
            line.label &&
            line.percent !== null && <span className="usage-island-line-label">{line.label}</span>
          )}
        </span>
      ))}
    </span>
  );
}

function UsageLines({ item, name }: { item: UsageIslandItem; name: string }) {
  return (
    <span className="usage-island-lines">
      <span className="usage-island-line-title">
        <SourceIcon icon={item.icon} />
        <span className="usage-island-line-name">{name}</span>
      </span>
      <span className="usage-island-line-quotas">
        {item.lines.map((line, index) => (
          <span
            key={index}
            className="usage-island-line"
            data-tone={quotaTone(line.percent)}
            data-bare={line.percent === null || undefined}
          >
            <span className="usage-island-line-head">
              {line.label && line.percent !== null && (
                <span className="usage-island-line-label">{line.label}</span>
              )}
              {line.resetAt != null && (
                <span className="usage-island-reset">
                  <QuotaCountdown resetAt={line.resetAt} compact small />
                </span>
              )}
              <span className="usage-island-line-value">{line.value}</span>
            </span>
            {line.percent !== null && (
              <span className="usage-island-line-track">
                <span style={{ width: `${line.percent}%` }} />
              </span>
            )}
          </span>
        ))}
      </span>
    </span>
  );
}

function Dial({
  items,
  displays,
  pick,
  onPick,
}: {
  items: UsageIslandItem[];
  displays: ItemDisplay[];
  pick: string | null;
  onPick?: (key: string | null) => void;
}) {
  const slice = (Math.PI * 2) / items.length;
  return items.map((item, index) => {
    const display = displays[index];
    const angle = -Math.PI / 2 + index * slice;
    return (
      <div key={item.key} role="listitem" className="usage-island-item">
        <button
          type="button"
          className="usage-island-dial-item"
          data-tone={display.tone}
          data-usage-source={item.key}
          aria-label={`${item.name} ${display.label} ${item.value}`}
          aria-pressed={item.key === pick}
          style={{
            left: Math.round(DIAL_CENTER + DIAL_ITEM_RADIUS * Math.cos(angle)),
            top: Math.round(DIAL_CENTER + DIAL_ITEM_RADIUS * Math.sin(angle)),
          }}
          onClick={() => onPick?.(item.key === pick ? null : item.key)}
        >
          <Gauge percent={item.percent} icon={item.icon} />
          <span className="usage-island-amount">{item.value}</span>
        </button>
      </div>
    );
  });
}

export function UsageIslandView({
  items,
  settings,
  pick = null,
  onPick,
  hover = null,
  masked = false,
  available,
  onLayout,
}: {
  items: UsageIslandItem[];
  settings: IslandSettings;
  pick?: string | null;
  onPick?: (key: string | null) => void;
  /** Set by hosts that track the cursor themselves. */
  hover?: boolean | null;
  /** The host already applied the privacy setting, as a separate window cannot follow it. */
  masked?: boolean;
  /** Fixed width for hosts that follow the island instead of containing it. */
  available?: number;
  onLayout?: (layout: IslandLayout) => void;
}) {
  const { t } = useI18n();
  const resolved = useThemeStore((theme) => theme.resolved);
  const hidden = useAccountPrivacyStore((privacy) => privacy.hidden);
  const [hovered, setHovered] = useState(false);
  const [pinned, setPinned] = useState(false);
  const { style, theme, expandMode } = settings;
  const reversed = settings.reversed.includes(style);
  const layout = islandLayout(style);
  const { root, tools, fit } = useIslandFit(layout);
  const width = available ?? fit.available;
  const picked = items.find((item) => item.key === pick);
  const systemDark = useSystemDark();
  const surface =
    theme === 'auto'
      ? resolved
      : theme === 'system' && !systemDark
        ? 'light'
        : theme === 'system' || theme === 'translucent'
          ? 'dark'
          : theme;
  const expanded =
    items.length > 0 &&
    (expandMode === 'always' ||
      (expandMode === 'click' && pinned) ||
      (expandMode === 'hover' && (hover ?? hovered)));
  const fullSize = {
    width:
      layout === 'dial' ? DIAL_SIZE : layout === 'column' ? LOGO_SIZE : fit.content + TOOLS_INSET,
    height:
      layout === 'dial'
        ? DIAL_SIZE
        : layout === 'column'
          ? LOGO_SIZE + items.length * COLUMN_ITEM_HEIGHT + 8
          : LOGO_SIZE,
  };
  const displays = items.map((item): ItemDisplay => {
    const account = item.kind === 'account';
    const label = account && hidden && !masked ? ACCOUNT_NAME_MASK : item.detail;
    return {
      label,
      name: account ? label || item.name : item.name,
      tone: quotaTone(item.percent),
    };
  });
  useEffect(() => {
    onLayout?.({
      expanded,
      width: fullSize.width,
      height: fullSize.height,
      anchorX: layout === 'dial' ? 0.5 : layout === 'row' && reversed ? 1 : 0,
      anchorY: layout === 'dial' ? 0.5 : layout === 'column' && reversed ? 1 : 0,
    });
  }, [onLayout, expanded, fullSize.width, fullSize.height, layout, reversed]);
  return (
    <div ref={root} className="usage-island-fit" data-layout={layout} data-reversed={reversed}>
      <div
        className="usage-island-track"
        style={
          {
            '--usage-island-width': `${fullSize.width}px`,
            '--usage-island-height': `${fullSize.height}px`,
            '--usage-island-tools-max': width ? `${width - TOOLS_INSET}px` : 'none',
          } as CSSProperties
        }
        data-layout={layout}
        data-reversed={reversed}
      >
        <div
          className={`usage-island usage-island-theme-${surface}${
            theme === 'auto'
              ? ' usage-island-theme-auto'
              : theme === 'translucent'
                ? ' usage-island-theme-translucent'
                : ''
          }`}
          data-layout={layout}
          data-reversed={reversed}
          data-expanded={expanded}
          data-density={style === 'line' ? lineDensity(width, items.length) : undefined}
          onPointerEnter={(event) => {
            if (event.pointerType === 'mouse') setHovered(true);
          }}
          onPointerLeave={(event) => {
            if (event.pointerType === 'mouse') setHovered(false);
          }}
        >
          {layout === 'dial' ? (
            <div className="usage-island-dial" role="list" aria-hidden={!expanded}>
              <Dial items={items} displays={displays} pick={pick} onPick={onPick} />
            </div>
          ) : (
            <div ref={tools} className="usage-island-tools" role="list" aria-hidden={!expanded}>
              {items.map((item, index) => {
                const display = displays[index];
                const content =
                  style === 'line' ? (
                    <UsageLines item={item} name={display.name} />
                  ) : (
                    <>
                      <Gauge percent={item.percent} icon={item.icon} />
                      {layout === 'column' ? (
                        <span className="usage-island-amount">{item.value}</span>
                      ) : (
                        <UsageRows item={item} />
                      )}
                    </>
                  );
                const label = `${item.name} ${display.label} ${item.value}`;
                return (
                  <div key={item.key} role="listitem" className="usage-island-item">
                    <button
                      type="button"
                      className="usage-island-tool"
                      data-tone={display.tone}
                      data-usage-source={item.key}
                      aria-label={label}
                      aria-pressed={item.key === pick}
                      onClick={() => onPick?.(item.key === pick ? null : item.key)}
                    >
                      {content}
                    </button>
                  </div>
                );
              })}
            </div>
          )}
          <button
            type="button"
            className="usage-island-toggle"
            aria-label={t(expanded ? 'usageIsland.collapse' : 'usageIsland.expand')}
            aria-expanded={expanded}
            disabled={!items.length || expandMode === 'always'}
            onClick={() => setPinned(!expanded)}
          >
            <span
              key={picked?.key ?? 'logo'}
              className="usage-island-logo"
              data-tone={picked ? quotaTone(picked.percent) : undefined}
            >
              {picked?.percent === null ? (
                <span className="usage-island-logo-value">
                  <SourceIcon icon={picked.icon} />
                  <span>{compactValue(picked.value)}</span>
                </span>
              ) : picked ? (
                <Gauge percent={picked.percent} icon={picked.icon} />
              ) : (
                <img src="/brand/bird.png" alt="" draggable={false} />
              )}
            </span>
          </button>
        </div>
      </div>
    </div>
  );
}

function ThumbnailMark({ x, y }: { x: number; y: number }) {
  return <circle cx={x} cy={y} r="5" fill="none" stroke="currentColor" strokeWidth="2" />;
}

export function UsageIslandThumbnail({
  style,
  reversed,
}: {
  style: UsageIslandStyle;
  reversed: boolean;
}) {
  const layout = islandLayout(style);
  if (layout === 'dial')
    return (
      <svg viewBox="0 0 120 64" aria-hidden="true">
        <circle cx="60" cy="32" r="30" fill="currentColor" opacity="0.14" />
        <circle cx="60" cy="32" r="7" fill="currentColor" />
        {[0, 1, 2, 3, 4, 5].map((index) => {
          const angle = -Math.PI / 2 + (index * Math.PI) / 3;
          return (
            <ThumbnailMark
              key={index}
              x={60 + 20 * Math.cos(angle)}
              y={32 + 20 * Math.sin(angle)}
            />
          );
        })}
      </svg>
    );
  if (layout === 'column')
    return (
      <svg viewBox="0 0 120 64" aria-hidden="true">
        <rect x="47" y="1" width="26" height="62" rx="13" fill="currentColor" opacity="0.14" />
        <circle cx="60" cy={reversed ? 51 : 13} r="7" fill="currentColor" />
        {(reversed ? [9, 28] : [28, 47]).map((y) => (
          <g key={y}>
            <ThumbnailMark x={60} y={y} />
            <rect x="55" y={y + 8} width="10" height="2.5" rx="1.25" fill="currentColor" />
          </g>
        ))}
      </svg>
    );
  return (
    <svg viewBox="0 0 120 64" aria-hidden="true">
      <rect x="2" y="18" width="116" height="28" rx="14" fill="currentColor" opacity="0.14" />
      <circle cx={reversed ? 104 : 16} cy="32" r="9" fill="currentColor" />
      {style === 'line'
        ? (reversed ? [10, 38, 66] : [32, 60, 88]).map((x) => (
            <g key={x}>
              <circle cx={x + 2} cy="27.5" r="2.5" fill="currentColor" opacity="0.7" />
              <rect x={x + 6} y="26" width="16" height="3" rx="1.5" fill="currentColor" />
              <rect x={x} y="35" width="22" height="3" rx="1.5" fill="currentColor" />
            </g>
          ))
        : (reversed ? [15, 43, 71] : [37, 65, 93]).map((x) => (
            <g key={x}>
              <ThumbnailMark x={x} y={32} />
              <rect
                x={x + 8}
                y="27"
                width="9"
                height="3"
                rx="1.5"
                fill="currentColor"
                opacity="0.6"
              />
              <rect
                x={x + 8}
                y="34"
                width="9"
                height="3"
                rx="1.5"
                fill="currentColor"
                opacity="0.6"
              />
            </g>
          ))}
    </svg>
  );
}
