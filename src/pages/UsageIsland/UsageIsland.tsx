import { useMemo, useState, type CSSProperties } from 'react';
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Box,
  Check,
  Plus,
  RefreshCw,
  RotateCcw,
  RotateCw,
  TriangleAlert,
  type LucideIcon,
} from 'lucide-react';
import { IslandScaleSlider } from '../../components/IslandScaleSlider';
import { MiniSelect } from '../../components/MiniSelect';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { useI18n } from '../../hooks/useI18n';
import type { TKey } from '../../i18n';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import {
  USAGE_ISLAND_LIMIT,
  usageIslandExpandModes,
  usageIslandRefreshSeconds,
  usageIslandScales,
  usageIslandStyles,
  usageIslandThemes,
  useUsageIslandStore,
  useUsageIslandVisible,
  type UsageIslandStyle,
} from '../../stores/usageIslandStore';
import { usageIslandDemoItems } from './usageIslandData';
import { UsageIslandSlots } from './UsageIslandSlots';
import { UsageIslandThumbnail, UsageIslandView } from './UsageIslandView';
import {
  useUsageIslandActive,
  useUsageIslandItems,
  useUsageIslandRefresh,
  useUsageSources,
} from './useUsageIslandData';
import '../FreeModels/FreeModels.css';
import '../VoiceInput/VoiceInput.css';
import './UsageIsland.css';

type Direction = { icon: LucideIcon; label: TKey };

// [forward, reversed] for each style.
const directions: Record<UsageIslandStyle, [Direction, Direction]> = {
  row: [
    { icon: ArrowRight, label: 'usageIsland.direction.right' },
    { icon: ArrowLeft, label: 'usageIsland.direction.left' },
  ],
  column: [
    { icon: ArrowDown, label: 'usageIsland.direction.down' },
    { icon: ArrowUp, label: 'usageIsland.direction.up' },
  ],
  dial: [
    { icon: RotateCw, label: 'usageIsland.direction.clockwise' },
    { icon: RotateCcw, label: 'usageIsland.direction.counterClockwise' },
  ],
  line: [
    { icon: ArrowRight, label: 'usageIsland.direction.right' },
    { icon: ArrowLeft, label: 'usageIsland.direction.left' },
  ],
};

export function UsageIslandMain() {
  const { t } = useI18n();
  const active = useUsageIslandActive();
  const config = useUsageIslandStore();
  const { update } = config;
  const sources = useUsageSources(active);
  const items = useUsageIslandItems(sources);
  const visible = useUsageIslandVisible();
  const { refreshing, refresh } = useUsageIslandRefresh(items, active && !visible);
  const [demoNow] = useState(() => Date.now() / 1000);
  const demoItems = useMemo(
    () => usageIslandDemoItems(config.style, demoNow),
    [config.style, demoNow]
  );
  const [demoPick, setDemoPick] = useState<string | null>(null);
  return (
    <div className="usage-island-editor flex-1 overflow-y-auto pr-1 pb-3">
      <div className="usage-island-page">
        <section
          className="usage-island-stage"
          aria-label={t('developer.usageIsland')}
          style={{ '--usage-island-scale': config.scale / 100 } as CSSProperties}
        >
          <div
            className="usage-island-preview"
            style={{ '--usage-island-scale': config.scale / 100 } as CSSProperties}
          >
            <UsageIslandView
              items={demoItems}
              settings={{ ...config, expandMode: 'always' }}
              pick={demoPick}
              onPick={setDemoPick}
            />
          </div>
        </section>
        <UsageIslandSlots sources={sources} />
        <div className="usage-island-lower">
          <div className="usage-island-styles">
            {usageIslandStyles.map((style) => {
              const reversed = config.reversed.includes(style);
              const styleLabel = t(`usageIsland.style.${style}` as TKey);
              return (
                <div key={style} className="usage-island-style-card">
                  <button
                    type="button"
                    className="usage-island-style rounded-card"
                    aria-label={styleLabel}
                    aria-pressed={config.style === style}
                    onClick={() => update({ style })}
                  >
                    <UsageIslandThumbnail style={style} reversed={reversed} />
                  </button>
                  <div
                    className="usage-island-direction"
                    role="group"
                    aria-label={styleLabel}
                    data-reversed={reversed}
                  >
                    {[true, false].map((option) => {
                      const { icon: Icon, label } = directions[style][option ? 1 : 0];
                      return (
                        <button
                          key={label}
                          type="button"
                          aria-label={t(label)}
                          aria-pressed={reversed === option}
                          onClick={() => {
                            if (reversed !== option) config.toggleReversed(style);
                          }}
                        >
                          <Icon size={18} aria-hidden="true" />
                        </button>
                      );
                    })}
                  </div>
                </div>
              );
            })}
          </div>
          <div className="voice-card rounded-card usage-island-settings">
            <div className="voice-row">
              <span>{t('voice.theme')}</span>
              <ViewModeTabs
                value={config.theme}
                onChange={(theme) => update({ theme })}
                options={usageIslandThemes.map((theme) => ({
                  value: theme,
                  label: t(`voice.${theme}` as TKey),
                }))}
              />
            </div>
            <div className="voice-row">
              <span>{t('usageIsland.expandMode')}</span>
              <ViewModeTabs
                value={config.expandMode}
                onChange={(expandMode) => update({ expandMode })}
                options={usageIslandExpandModes.map((mode) => ({
                  value: mode,
                  label: t(`usageIsland.expandMode.${mode}` as TKey),
                }))}
              />
            </div>
            <div className="voice-row">
              <span>{t('usageIsland.refresh')}</span>
              <span className="usage-island-refresh">
                <button
                  type="button"
                  className="voice-text-button"
                  disabled={refreshing || !items.length}
                  onClick={() => void refresh(true)}
                >
                  <RefreshCw size={14} className={refreshing ? 'animate-spin' : ''} />
                  {t('accountCenter.refreshAll')}
                </button>
                <MiniSelect
                  value={String(config.refreshSeconds)}
                  dropUp
                  onChange={(value) => update({ refreshSeconds: Number(value) })}
                  options={usageIslandRefreshSeconds.map((seconds) => ({
                    id: String(seconds),
                    label: seconds
                      ? t('usageIsland.refresh.minutes').replace('{n}', String(seconds / 60))
                      : t('usageIsland.refresh.manual'),
                  }))}
                />
              </span>
            </div>
            <div className="voice-row">
              <IslandScaleSlider
                scales={usageIslandScales}
                value={config.scale}
                label={t('usageIsland.scale')}
                onChange={(scale) => update({ scale })}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function UsageSourceRow({
  name,
  detail,
  icon,
  value,
  missing,
  selected,
  disabled,
  onToggle,
}: {
  name: string;
  detail: string;
  icon: string | null;
  value?: string;
  missing?: boolean;
  selected: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  return (
    <div
      className={`relative overflow-hidden h-14 shrink-0 px-[11px] py-1 rounded-card transition-colors flex items-center gap-3 border-2 ${
        selected
          ? 'border-cyber-accent/70 bg-cyber-accent/10'
          : 'border-transparent bg-cyber-surface/60 hover:bg-cyber-elevated/70'
      }`}
    >
      <button
        type="button"
        aria-label={`${name} — ${detail}`}
        aria-pressed={selected}
        disabled={disabled}
        onClick={onToggle}
        className="absolute inset-0 rounded-card focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-cyber-accent"
      />
      <div className="relative z-10 pointer-events-none flex items-center gap-3 shrink-0">
        {selected ? (
          <Check size={22} className="text-cyber-accent" />
        ) : (
          <Plus size={22} className={disabled ? 'opacity-40' : 'text-cyber-text-muted'} />
        )}
        {missing ? (
          <TriangleAlert size={32} className="text-cyber-text-muted" />
        ) : icon ? (
          <img src={icon} alt="" className="w-8 h-8 object-contain" />
        ) : (
          <Box size={32} className="text-cyber-text" />
        )}
      </div>
      <div className="relative z-10 pointer-events-none flex-1 min-w-0 grid grid-cols-[minmax(0,1fr)] auto-rows-[16px] items-center">
        <div className="flex items-center gap-2">
          <div className="text-sm font-bold truncate leading-none flex-1 min-w-0">{name}</div>
          {value && (
            <span className="text-[10px] text-cyber-text-secondary shrink-0 whitespace-nowrap tabular-nums">
              {value}
            </span>
          )}
        </div>
        <div className="text-[10px] leading-tight truncate text-cyber-text-secondary/70">
          {detail}
        </div>
      </div>
    </div>
  );
}

export function UsageIslandPanel() {
  const { t } = useI18n();
  const active = useUsageIslandActive();
  const hidden = useAccountPrivacyStore((privacy) => privacy.hidden);
  const selected = useUsageIslandStore((state) => state.sources);
  const toggleSource = useUsageIslandStore((state) => state.toggleSource);
  const sources = useUsageSources(active);
  const [tab, setTab] = useState<'account' | 'model'>('account');
  const rows = sources.filter((source) => source.kind === tab);
  const missing = selected.filter(
    (key) => key.startsWith(`${tab}:`) && !sources.some((source) => source.key === key)
  );
  const full = selected.length >= USAGE_ISLAND_LIMIT;
  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="h-10 shrink-0 px-2 mb-2 flex items-center justify-between gap-2">
        <div className="min-w-0 flex-1">
          <ViewModeTabs
            fullWidth
            value={tab}
            onChange={setTab}
            options={[
              { value: 'account', label: t('usageIsland.accounts') },
              { value: 'model', label: t('usageIsland.models') },
            ]}
          />
        </div>
      </div>
      <div className="flex-1 px-2 pt-0.5 pb-2 overflow-y-auto space-y-2">
        {rows.map((source) => {
          const isSelected = selected.includes(source.key);
          return (
            <UsageSourceRow
              key={source.key}
              name={source.name}
              detail={source.kind === 'account' && hidden ? ACCOUNT_NAME_MASK : source.detail}
              icon={source.icon}
              value={source.value}
              selected={isSelected}
              disabled={!isSelected && full}
              onToggle={() => toggleSource(source.key)}
            />
          );
        })}
        {missing.map((key) => (
          <UsageSourceRow
            key={key}
            name={t('voice.missingSource')}
            detail=""
            icon={null}
            missing
            selected
            disabled={false}
            onToggle={() => toggleSource(key)}
          />
        ))}
        {!rows.length && !missing.length && (
          <div className="flex h-12 items-center justify-center rounded-card bg-cyber-surface/60 text-xs text-cyber-text-muted">
            {t(tab === 'account' ? 'accountCenter.empty' : 'usageIsland.noModels')}
          </div>
        )}
      </div>
    </div>
  );
}
