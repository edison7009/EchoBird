import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { Box, Gift, RefreshCw, Zap } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useSystemDark } from '../../hooks/useSystemDark';
import { useThemeStore } from '../../stores/themeStore';
import { useBackgroundStore, type BackgroundMode } from '../../stores/backgroundStore';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import type { UsageIslandConfig, UsageIslandStyle } from '../../stores/usageIslandStore';
import { quotaTone } from '../../utils/quotaColors';
import { QuotaCountdown } from '../AppManager/QuotaCountdown';
import { compactValue, type UsageIslandAction, type UsageIslandItem } from './usageIslandData';

const LOGO_SIZE = 48;
const COLUMN_ITEM_HEIGHT = 54;
const LIST_ITEM_HEIGHT = 52;
const LIST_ITEM_GAP = 6;
// Extra space under the logo so it sits as far from the first row as rows do apart.
const LIST_HEADER_EXTRA = 8;
const LIST_WIDTH = 280;
// Logo slot plus the end padding, and the gap between sources; both mirror the CSS.
const TOOLS_INSET = 60;
// Line style: the Smart Access band bleeds 10px past its column, so the tools
// start 64px in and end 20px from the edge, leaving the band 10px clear of the
// 44px logo button and of the island edge.
const LINE_TOOLS_INSET = 84;
// Line-style column gap; keep in sync with UsageIsland.css.
const TOOLS_GAP = 28;
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

export type IslandSettings = Pick<
  UsageIslandConfig,
  'style' | 'reversed' | 'theme' | 'expandMode'
> &
  Partial<Pick<UsageIslandConfig, 'opacity' | 'showSmartAccess'>> & {
    /** App background; the floating window cannot read the main window's store. */
    background?: BackgroundMode;
  };

export interface IslandLayout {
  expanded: boolean;
  width: number;
  height: number;
  radius?: number;
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
        // Use layout width so unrelated visual transforms do not affect fitting.
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
  const share = (available - LINE_TOOLS_INSET - TOOLS_GAP * (count - 1)) / count;
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

function Gauge({
  percent,
  icon,
  smartActive = false,
}: {
  percent: number | null;
  icon: string | null;
  smartActive?: boolean;
}) {
  return (
    <span className="usage-island-gauge" data-smart-active={smartActive || undefined}>
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

function UsageLines({
  item,
  name,
  icon = true,
}: {
  item: UsageIslandItem;
  name: string;
  icon?: boolean;
}) {
  return (
    <span className="usage-island-lines">
      <span className="usage-island-line-title">
        {icon && <SourceIcon icon={item.icon} />}
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
          <Gauge percent={item.percent} icon={item.icon} smartActive={item.smartActive} />
          <span className="usage-island-amount">{item.value}</span>
        </button>
      </div>
    );
  });
}

export interface UsageIslandActions {
  warmUp: boolean;
  claim: boolean;
  refresh: boolean;
  blocked: boolean;
  running: UsageIslandAction | null;
}

function ListActions({
  actions,
  onAction,
}: {
  actions?: UsageIslandActions;
  onAction?: (action: UsageIslandAction) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="usage-island-list-actions">
      {(
        [
          ['warmUp', Zap, 'agent.warmUpAccount'],
          ['claim', Gift, 'accountCenter.claimAll'],
          ['refresh', RefreshCw, 'btn.refresh'],
        ] as const
      ).map(
        ([action, Icon, label]) =>
          (action === 'refresh' || actions?.[action]) && (
            <button
              key={action}
              type="button"
              aria-label={t(label)}
              disabled={!actions?.[action] || actions.blocked || !onAction}
              onClick={(event) => {
                event.stopPropagation();
                onAction?.(action);
              }}
            >
              <Icon
                size={14}
                aria-hidden="true"
                className={
                  actions?.running === action
                    ? action === 'refresh'
                      ? 'animate-spin'
                      : 'animate-pulse'
                    : undefined
                }
              />
            </button>
          )
      )}
    </div>
  );
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
  onAction,
  actions,
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
  onAction?: (action: UsageIslandAction) => void;
  actions?: UsageIslandActions;
}) {
  const { t } = useI18n();
  const resolved = useThemeStore((theme) => theme.resolved);
  const hidden = useAccountPrivacyStore((privacy) => privacy.hidden);
  const [hovered, setHovered] = useState(false);
  const [pinned, setPinned] = useState(false);
  const appBackground = useBackgroundStore((state) => state.mode);
  const { style, theme, expandMode } = settings;
  const background = settings.background ?? appBackground;
  // The theme surface carries the app's static texture; Character Flow is an
  // animated scene that does not suit the island, so it stays solid.
  const texture =
    theme === 'auto' &&
    (background === 'carbon' || background === 'basalt' || background === 'monogram')
      ? background
      : null;
  const effectiveTheme = (theme as string) === 'translucent' ? 'dark' : theme;
  const reversed = settings.reversed.includes(style);
  const layout = islandLayout(style);
  const toolsInset = style === 'line' ? LINE_TOOLS_INSET : TOOLS_INSET;
  const { root, tools, fit } = useIslandFit(layout);
  const width = available ?? fit.available;
  const picked = items.find((item) => item.key === pick);
  const systemDark = useSystemDark();
  const surface =
    effectiveTheme === 'auto'
      ? resolved
      : effectiveTheme === 'system' && !systemDark
        ? 'light'
        : effectiveTheme === 'system'
          ? 'dark'
          : effectiveTheme;
  const expanded =
    items.length > 0 &&
    (expandMode === 'always' ||
      (expandMode === 'click' && pinned) ||
      (expandMode === 'hover' && (hover ?? hovered)));
  const vertical = layout === 'column' || layout === 'list';
  const fullSize = {
    width:
      layout === 'dial'
        ? DIAL_SIZE
        : layout === 'list'
          ? Math.min(LIST_WIDTH, width || LIST_WIDTH)
          : layout === 'column'
            ? LOGO_SIZE
            : fit.content + toolsInset,
    height:
      layout === 'dial'
        ? DIAL_SIZE
        : vertical
          ? LOGO_SIZE +
            items.length * (layout === 'list' ? LIST_ITEM_HEIGHT : COLUMN_ITEM_HEIGHT) +
            (layout === 'list'
              ? Math.max(0, items.length - 1) * LIST_ITEM_GAP + 12 + LIST_HEADER_EXTRA
              : 8)
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
      ...(layout === 'list' ? { radius: 24 } : {}),
      anchorX: layout === 'dial' ? 0.5 : layout === 'row' && reversed ? 1 : 0,
      anchorY: layout === 'dial' ? 0.5 : vertical && reversed ? 1 : 0,
    });
  }, [onLayout, expanded, fullSize.width, fullSize.height, layout, reversed, vertical]);
  return (
    <div ref={root} className="usage-island-fit" data-layout={layout} data-reversed={reversed}>
      <div
        className="usage-island-track"
        style={
          {
            '--usage-island-width': `${fullSize.width}px`,
            '--usage-island-height': `${fullSize.height}px`,
            '--usage-island-tools-max': width ? `${width - toolsInset}px` : 'none',
            '--usage-island-surface-opacity': `${settings.opacity ?? 100}%`,
          } as CSSProperties
        }
        data-layout={layout}
        data-reversed={reversed}
      >
        <div
          className={`usage-island usage-island-theme-${surface}${
            theme === 'auto' ? ' usage-island-theme-auto' : ''
          }`}
          data-layout={layout}
          data-reversed={reversed}
          data-expanded={expanded}
          data-style={style}
          data-density={style === 'line' ? lineDensity(width, items.length) : undefined}
          onPointerEnter={(event) => {
            if (event.pointerType === 'mouse') setHovered(true);
          }}
          onPointerLeave={(event) => {
            if (event.pointerType === 'mouse') setHovered(false);
          }}
        >
          {texture && (
            <span
              aria-hidden="true"
              data-island-texture={texture}
              className={`usage-island-texture ${
                texture === 'basalt'
                  ? 'app-background-basalt'
                  : `app-background-material app-background-${texture}`
              }`}
            />
          )}
          {layout === 'list' && expanded && <ListActions actions={actions} onAction={onAction} />}
          {layout === 'dial' ? (
            <div className="usage-island-dial" role="list" aria-hidden={!expanded}>
              <Dial items={items} displays={displays} pick={pick} onPick={onPick} />
            </div>
          ) : (
            <div ref={tools} className="usage-island-tools" role="list" aria-hidden={!expanded}>
              {items.map((item, index) => {
                const display = displays[index];
                const content =
                  style === 'line' || style === 'list' ? (
                    <>
                      {style === 'list' && <SourceIcon icon={item.icon} />}
                      <UsageLines item={item} name={display.name} icon={style !== 'list'} />
                    </>
                  ) : (
                    <>
                      <Gauge
                        percent={item.percent}
                        icon={item.icon}
                        smartActive={item.smartActive}
                      />
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
                      data-smart-active={item.smartActive || undefined}
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
            aria-label={
              picked ? picked.name : t(expanded ? 'usageIsland.collapse' : 'usageIsland.expand')
            }
            aria-pressed={picked ? true : undefined}
            aria-expanded={expanded}
            disabled={!items.length || (expandMode === 'always' && !picked)}
            onClick={() => {
              // A picked source shown in the logo slot is cleared first; the
              // next click goes back to expanding or collapsing.
              if (picked) onPick?.(null);
              else setPinned(!expanded);
            }}
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
                <Gauge
                  percent={picked.percent}
                  icon={picked.icon}
                  smartActive={picked.smartActive}
                />
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
  if (layout === 'list')
    return (
      <svg viewBox="0 0 120 64" aria-hidden="true">
        <rect x="30" y="1" width="60" height="62" rx="10" fill="currentColor" opacity="0.14" />
        <circle cx="40" cy={reversed ? 54 : 10} r="5" fill="currentColor" />
        <RefreshCw x={74} y={reversed ? 49 : 5} size={10} style={{ width: 10, height: 10 }} />
        {(reversed ? [10, 27] : [27, 44]).map((y) => (
          <g key={y}>
            <circle cx="40" cy={y} r="3" fill="currentColor" opacity="0.7" />
            <rect x="47" y={y - 2} width="32" height="3" rx="1.5" fill="currentColor" />
            <rect x="37" y={y + 6} width="42" height="2" rx="1" fill="currentColor" />
          </g>
        ))}
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
