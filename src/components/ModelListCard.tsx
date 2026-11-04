import type { ReactNode } from 'react';
import { Box, Waypoints, RefreshCw, Server, SquarePen, Trash2 } from 'lucide-react';
import type { ModelConfig } from '../api/types';
import type { ModelUsageData } from '../api/tauri';
import type { TKey } from '../i18n';
import { getModelIcon } from './cards';
import { ModelQuotaSummary } from './ModelQuotaSummary';
import { isSmartRouterId } from '../utils/smartRouter';

interface ModelListCardProps {
  model: ModelConfig;
  selection: ReactNode;
  onSelect: () => void;
  selectionLabel?: string;
  selectionDisabled?: boolean;
  selected?: boolean;
  disabled?: boolean;
  compact?: boolean;
  subtitle?: string;
  badge?: ReactNode;
  secondaryBadge?: ReactNode;
  usage?: ModelUsageData;
  refreshing?: boolean;
  onRefreshUsage?: (modelId: string) => void;
  onEditModel?: (model: ModelConfig) => void;
  onDeleteModel?: (modelId: string) => void;
  t: (key: TKey) => string;
}

export function ModelListCard({
  model,
  selection,
  onSelect,
  selectionLabel,
  selectionDisabled = false,
  selected = false,
  disabled = false,
  compact = false,
  subtitle = model.modelId || '',
  badge,
  secondaryBadge,
  usage,
  refreshing = false,
  onRefreshUsage,
  onEditModel,
  onDeleteModel,
  t,
}: ModelListCardProps) {
  const isSmartRouter = isSmartRouterId(model.internalId);
  const isLocalModel = model.internalId === 'local-server' || isSmartRouter;
  const canManage = !isLocalModel && model.modelType !== 'DEMO';
  const iconSrc = isSmartRouter ? null : getModelIcon('', model.modelId || '');

  return (
    <div
      aria-disabled={disabled || undefined}
      className={`relative overflow-hidden ${compact ? 'h-12 px-[11px] py-1' : 'p-[11px]'} rounded-card transition-colors flex items-center gap-3 border-2 ${
        disabled ? 'opacity-50' : ''
      } ${selected ? 'border-cyber-accent/70 bg-cyber-accent/10' : 'border-transparent bg-cyber-surface/60 hover:bg-cyber-elevated/70'}`}
    >
      <button
        type="button"
        aria-label={selectionLabel ?? `${model.name} — ${subtitle}`}
        disabled={disabled || selectionDisabled}
        onClick={onSelect}
        className={`absolute inset-0 rounded-card focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-cyber-accent ${disabled ? 'cursor-not-allowed' : 'disabled:cursor-default'}`}
      />
      <div className="relative z-10 pointer-events-none flex items-center gap-3 shrink-0">
        {selection}
        {iconSrc ? (
          <img
            src={iconSrc}
            alt=""
            className="w-6 h-6"
            onError={(event) => {
              event.currentTarget.style.display = 'none';
            }}
          />
        ) : (
          <div
            className={`w-6 h-6 flex items-center justify-center ${isLocalModel ? 'text-cyber-accent' : 'text-cyber-text'}`}
          >
            {isSmartRouter ? (
              <Waypoints size={22} />
            ) : isLocalModel ? (
              <Server size={22} />
            ) : (
              <Box size={22} />
            )}
          </div>
        )}
      </div>
      <div
        className={`relative z-10 pointer-events-none flex-1 min-w-0 ${compact ? 'grid grid-cols-[minmax(0,1fr)] auto-rows-[16px] items-center' : 'flex flex-col justify-center min-h-[2.5rem] py-0.5'}`}
      >
        <div className="flex items-center gap-2">
          <div className="text-sm font-bold truncate leading-none flex-1 min-w-0">
            {model.name || 'Untitled Model'}
          </div>
          {!isLocalModel && usage && <ModelQuotaSummary usage={usage} t={t} />}
          {badge}
        </div>
        <div
          className={`flex items-center gap-2 ${compact ? 'h-[16px]' : 'mt-1'} text-[10px] leading-tight`}
        >
          <span className="flex-1 min-w-0 truncate text-cyber-text-secondary/70">{subtitle}</span>
          {secondaryBadge}
          {!isLocalModel && (onRefreshUsage || (canManage && (onDeleteModel || onEditModel))) && (
            <div className="pointer-events-auto flex items-center gap-1 shrink-0">
              {onRefreshUsage && (
                <button
                  type="button"
                  className="p-0.5 text-cyber-text-muted/70 hover:text-cyber-text transition-colors"
                  aria-label={t('btn.refreshUsage')}
                  aria-busy={refreshing}
                  onClick={(event) => {
                    event.stopPropagation();
                    if (!refreshing) onRefreshUsage(model.internalId);
                  }}
                >
                  <RefreshCw size={12} className={refreshing ? 'animate-spin' : ''} />
                </button>
              )}
              {canManage && onDeleteModel && (
                <button
                  type="button"
                  className="p-0.5 text-cyber-text-muted/70 hover:text-red-500 transition-colors"
                  aria-label={t('btn.delete')}
                  onClick={(event) => {
                    event.stopPropagation();
                    onDeleteModel(model.internalId);
                  }}
                >
                  <Trash2 size={12} />
                </button>
              )}
              {canManage && onEditModel && (
                <button
                  type="button"
                  className="p-0.5 text-cyber-text-muted/70 hover:text-cyber-text transition-colors"
                  aria-label={t('btn.edit')}
                  onClick={(event) => {
                    event.stopPropagation();
                    onEditModel(model);
                  }}
                >
                  <SquarePen size={12} />
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
