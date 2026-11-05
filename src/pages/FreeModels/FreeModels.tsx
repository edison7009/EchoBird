import {
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentPropsWithRef,
  type RefObject,
  type PointerEventHandler,
} from 'react';
import { createPortal } from 'react-dom';
import { accountProxySources } from '../../hooks/useAccountPlugin';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  KeyboardSensor,
  closestCenter,
  pointerWithin,
  rectIntersection,
  useDndContext,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type CollisionDetection,
  type Modifier,
} from '@dnd-kit/core';
import {
  SortableContext,
  useSortable,
  rectSortingStrategy,
  verticalListSortingStrategy,
  sortableKeyboardCoordinates,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import {
  Box,
  Check,
  CircleHelp,
  ExternalLink,
  KeyRound,
  GripVertical,
  LayoutGrid,
  List,
  Terminal,
  Plus,
  RefreshCw,
  SquarePen,
  Waypoints,
  Trash2,
  X,
} from 'lucide-react';
import * as api from '../../api/tauri';
import * as modelApi from '../../api/models';
import {
  accountProviderIconSrc,
  accountModelPickerOptions,
  cacheAccountProxyModels,
  getAccountProxyModels,
  getCachedAccountProxyModels,
  listProxyAccounts,
  type ProxyAccount,
} from '../../api/accountProxy';
import type { AccountSource } from '../../api/types';
import type { FreeModelEntry } from '../../api/freeModels';
import { getModelIcon } from '../../components';
import { ModelListCard } from '../../components/ModelListCard';
import { LatencyIndicator } from '../../components/LatencyIndicator';
import { RoutingToggle } from '../../components/RoutingToggle';
import { ModelIdCombobox } from '../../components/ModelIdCombobox';
import { AccountPrivacyButton } from '../../components/AccountPrivacyButton';
import { PageActionButton } from '../../components/PageActionButton';
import { ViewModeTabGroup, type ViewModeTabPosition } from '../../components/ViewModeTabGroup';
import { useConfirm } from '../../components/ConfirmDialog';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import { useDocumentVisible } from '../../hooks/useDocumentVisible';
import { useNavigationStore } from '../../stores/navigationStore';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { AccountProxyPanel, openAccountModelForm } from './AccountProxyPanel';
import { useAccountAccessRisk } from '../../stores/accountAccessRiskStore';
import { modelLatencyColor } from '../../utils/modelLatency';
import { useModelNexus } from '../ModelNexus/context';
import { accountProxyIdentityLabel, accountProxyPlanLabel } from '../AppManager/accountPlanLabel';
import { accountProxyStatus } from '../AppManager/accountProxyStatus';
import { AppManagerContext } from '../AppManager/context';
import './FreeModels.css';
import { type RouteModelNode, providerRank, useFreeModels } from './provider';
import { useRouteResponseTests } from './useRouteResponseTests';
import { RouterTerminal } from './RouterTerminal';
import { RouterGroups } from './RouterGroups';
export { FreeModelsProvider, useFreeModels } from './provider';

const NODE_COLUMN_GAP = 24;
const NODE_MIN_WIDTH = 240;

const NODE_ROW_GAP = 36;

const NODE_SIDE_PADDING = 0;

const HUB_TO_NODE_GAP = 64;

const HUB_ARROW_GAP = 4;

const HUB_ARROW_HEIGHT = 12;

const HUB_ARROW_LINE_GAP = 2;

const ROUTER_ACTIVITY_POLL_MS = 750;

const ROUTER_ACTIVITY_GRACE_MS = 2200;

const ROUTER_REMOVE_DROP_ID = 'router-remove-drop-zone';

const restrictRouteToVerticalAxis: Modifier = ({ transform }) => ({ ...transform, x: 0 });

const routeCollisionDetection: CollisionDetection = (args) => {
  const removeTargets = args.droppableContainers.filter(
    (entry) => entry.id === ROUTER_REMOVE_DROP_ID
  );
  const removeHits = (args.pointerCoordinates ? pointerWithin : rectIntersection)({
    ...args,
    droppableContainers: removeTargets,
  });
  if (removeHits.length > 0) return removeHits;
  return closestCenter({
    ...args,
    droppableContainers: args.droppableContainers.filter(
      (entry) => entry.id !== ROUTER_REMOVE_DROP_ID
    ),
  });
};

interface FreeModelProviderGroup {
  id: string;
  name: string;
  baseUrl: string;
  docsUrl: string;
  models: FreeModelEntry[];
}

interface RoutePath {
  id: string;
  d: string;
}

interface RouteArrow {
  x: number;
  y: number;
}

function routeBranchToBus(x: number, top: number, busY: number, hubX: number): string {
  const dx = hubX - x;
  if (Math.abs(dx) < 1) return `V ${busY}`;
  const radius = Math.min(12, (top - busY) / 2, Math.abs(dx) / 2);
  return `V ${busY + radius} A ${radius} ${radius} 0 0 ${dx > 0 ? 1 : 0} ${x + Math.sign(dx) * radius} ${busY} H ${hubX}`;
}

function isKeylessFreeModel(model: FreeModelEntry): boolean {
  return /^Leave the API key empty\./.test(model.notes);
}

export function FreeModelsTitleActions() {
  const { t } = useI18n();
  const { layout, setLayout, activeDragId } = useFreeModels();
  const [showHelp, setShowHelp] = useState(false);

  useEffect(() => {
    if (!showHelp) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setShowHelp(false);
    };
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, [showHelp]);

  return (
    <div className="flex shrink-0 items-center gap-3">
      <AccountPrivacyButton />
      <ViewModeTabGroup
        role="group"
        aria-label={t('freeModels.layout.label')}
        className="view-mode-tabs flex h-9 overflow-hidden rounded-button border border-cyber-border"
      >
        {(['cards', 'list', 'terminal'] as const).map((mode) => {
          const Icon = mode === 'cards' ? LayoutGrid : mode === 'list' ? List : Terminal;
          return (
            <button
              key={mode}
              type="button"
              aria-label={t(
                mode === 'cards'
                  ? 'freeModels.layout.cards'
                  : mode === 'list'
                    ? 'freeModels.layout.list'
                    : 'freeModels.layout.terminal'
              )}
              aria-pressed={layout === mode}
              disabled={activeDragId !== null}
              onClick={() => setLayout(mode)}
              className={`view-mode-tab flex w-9 items-center justify-center transition-[color] ${layout === mode ? 'bg-cyber-elevated text-cyber-text' : 'text-cyber-text-secondary hover:text-cyber-text'}`}
            >
              <Icon size={16} aria-hidden="true" />
            </button>
          );
        })}
      </ViewModeTabGroup>
      <PageActionButton icon={CircleHelp} onClick={() => setShowHelp(true)} className="font-mono">
        {t('freeModels.help')}
      </PageActionButton>

      {showHelp && (
        <div className="fixed inset-0 z-[9998] flex items-center justify-center">
          <div className="absolute inset-0 bg-black/60" onClick={() => setShowHelp(false)} />
          <div className="relative w-[520px] max-w-[90vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden">
            <div className="h-px w-full bg-cyber-border" />
            <button
              type="button"
              onClick={() => setShowHelp(false)}
              aria-label={t('btn.close')}
              className="absolute right-5 top-5 text-cyber-text-secondary close-button transition-colors"
            >
              <X size={18} />
            </button>

            <div className="px-6 pt-6 pb-6 pr-14 space-y-4">
              <div className="flex gap-3">
                <Waypoints size={18} className="mt-0.5 flex-shrink-0 text-cyber-accent" />
                <div>
                  <div className="text-sm font-semibold text-cyber-text">
                    {t('freeModels.help.routerTitle')}
                  </div>
                  <p className="mt-1 text-xs leading-5 text-cyber-text-secondary">
                    {t('freeModels.help.routerDesc')}
                  </p>
                </div>
              </div>

              <div className="h-px bg-cyber-border/60" />

              <div className="flex gap-3">
                <KeyRound size={18} className="mt-0.5 flex-shrink-0 text-cyber-accent" />
                <div>
                  <div className="text-sm font-semibold text-cyber-text">
                    {t('freeModels.help.useTitle')}
                  </div>
                  <ol className="mt-1 space-y-1.5 text-xs leading-5 text-cyber-text-secondary list-decimal list-inside">
                    <li>{t('freeModels.help.step1')}</li>
                    <li>{t('freeModels.help.step2')}</li>
                    <li>{t('freeModels.help.step3')}</li>
                  </ol>
                </div>
              </div>
            </div>

            <button
              type="button"
              onClick={() => setShowHelp(false)}
              className="w-full border-t border-cyber-border px-4 py-3 text-[14px] font-semibold text-cyber-text hover:bg-cyber-text/10 transition-colors"
            >
              {t('btn.close')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

interface RouteModelCardProps {
  model: RouteModelNode;
  account?: ProxyAccount;
  keyless?: boolean;
  providerIconSrc?: string | null;
  priority: number;
  active?: boolean;
  compact?: boolean;
  modelIdOptions?: string[];
  freeModelIds?: string[];
  paidModelIds?: string[];
  optionMultipliers?: Record<string, number>;
  onLoadModels?: () => void;
  refreshing?: boolean;
  saving?: boolean;
  dragging?: boolean;
  onModelChange?: (id: string, modelId: string) => Promise<void>;
  list?: boolean;
  paused?: boolean;
  pending?: boolean;
  statusLabel?: string;
  testing?: boolean;
  testResult?: api.SmartRouterProbeResult;
  authorizationFailed?: boolean;
  reauthorizing?: boolean;
  onReauthorize?: () => void;
  onEnabledChange?: (enabled: boolean) => void;
  onEdit?: () => void;
  onRemove?: () => void;
}

function RouteModelCard({
  model,
  account,
  keyless = false,
  providerIconSrc,
  priority,
  active = false,
  compact = false,
  modelIdOptions = [],
  freeModelIds,
  paidModelIds,
  optionMultipliers,
  onLoadModels,
  refreshing = false,
  saving = false,
  dragging = false,
  onModelChange,
  list = false,
  paused = false,
  pending = false,
  statusLabel,
  testing = false,
  testResult,
  authorizationFailed = false,
  reauthorizing = false,
  onReauthorize,
  onEnabledChange,
  onEdit,
  onRemove,
  dragHandleProps,
  overlay = false,
}: RouteModelCardProps & {
  dragHandleProps?: ComponentPropsWithRef<'button'>;
  overlay?: boolean;
}) {
  const { t } = useI18n();
  const hideAccountNames = useAccountPrivacyStore((state) => state.hidden);
  const [pickerOpen, setPickerOpen] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);
  const dragPointerDown: PointerEventHandler<HTMLElement> | undefined =
    dragHandleProps?.onPointerDown;
  const accountProvider = accountProxySources().find((p) => p.id === model.accountSource?.provider);
  const accountProviderName = accountProvider?.name ?? model.provider;
  const displayName = model.accountSource
    ? hideAccountNames
      ? ACCOUNT_NAME_MASK
      : (account && accountProxyIdentityLabel(account, t)) ||
        accountProvider?.name ||
        model.provider
    : model.provider;
  const displayPlan = account ? accountProxyPlanLabel(account, t).trim() : '';
  const iconSrc = accountProvider
    ? accountProviderIconSrc(accountProvider.icon)
    : providerIconSrc !== undefined
      ? providerIconSrc
      : getModelIcon(model.provider, model.modelId);
  const hasModelPicker = Boolean(onModelChange && modelIdOptions.length > 0 && !overlay);
  if (pickerOpen && (!hasModelPicker || saving || dragging || pending)) setPickerOpen(false);
  const changePickerOpen = (open: boolean) => {
    if (open && (!hasModelPicker || saving || dragging || pending)) return;
    setPickerOpen(open);
    if (open && !pickerOpen) onLoadModels?.();
  };
  const modelPicker = hasModelPicker ? (
    <div
      className="free-model-route-picker pointer-events-auto min-w-0"
      aria-busy={saving || refreshing}
      onPointerDown={(event) => {
        event.stopPropagation();
        if (event.currentTarget.contains(event.target as Node)) dragPointerDown?.(event);
      }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      <ModelIdCombobox
        compact
        showSelectedFreeLabel
        selectOnly
        value={model.modelId}
        options={modelIdOptions}
        optionLabels={account?.modelLabels}
        freeModelIds={account?.freeModelIds ?? freeModelIds}
        paidModelIds={paidModelIds}
        optionMultipliers={optionMultipliers}
        freeLabel={t('model.free')}
        refreshing={refreshing}
        disabled={saving || dragging || pending}
        open={pickerOpen}
        onOpenChange={changePickerOpen}
        externalTriggerRef={list ? undefined : cardRef}
        ariaLabel={`${t('model.modelId')}: ${displayName} (${priority})`}
        onChange={(modelId) => void onModelChange?.(model.id, modelId)}
      />
    </div>
  ) : (
    <span className="block min-w-0 text-xs text-cyber-text-secondary truncate">
      {account?.modelLabels?.[model.modelId] ?? model.modelId}
    </span>
  );
  const activator = (dragHandleProps || overlay) && (
    <button
      {...dragHandleProps}
      type="button"
      tabIndex={overlay ? -1 : dragHandleProps?.tabIndex}
      disabled={list ? pending || saving || overlay || authorizationFailed : undefined}
      aria-label={`${t('freeModels.router.priority')} ${priority}: ${model.modelId}`}
      aria-pressed={list ? !paused : undefined}
      aria-expanded={!list && hasModelPicker ? pickerOpen : undefined}
      onPointerDown={dragPointerDown}
      onClick={() => {
        if (saving || dragging || pending || overlay || authorizationFailed) return;
        if (list) onEnabledChange?.(paused);
        else if (hasModelPicker) changePickerOpen(!pickerOpen);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter') {
          event.preventDefault();
          if (saving || dragging || pending || overlay || authorizationFailed) return;
          if (list) onEnabledChange?.(paused);
          else if (hasModelPicker) changePickerOpen(true);
        } else {
          dragHandleProps?.onKeyDown?.(event);
        }
      }}
      className={`absolute inset-0 rounded-card ${list ? 'touch-pan-y' : 'touch-none'} focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-cyber-accent`}
    />
  );

  if (list) {
    return (
      <div
        ref={cardRef}
        aria-hidden={overlay || undefined}
        className={`free-model-route-row relative rounded-card ${active ? 'is-active' : ''} ${paused ? 'is-paused' : ''} ${overlay ? 'shadow-2xl' : ''}`}
      >
        {activator}
        <div className="free-model-route-row-identity relative pointer-events-none flex min-w-0 items-center">
          <span className="flex w-6 shrink-0 items-center justify-center" aria-hidden="true">
            <GripVertical size={14} className="text-cyber-text-muted" />
          </span>
          <span className="free-model-route-row-number w-6 shrink-0 text-center text-sm font-semibold tabular-nums text-cyber-accent">
            {priority}
          </span>
          {iconSrc ? (
            <img src={iconSrc} alt="" className="h-6 w-6 shrink-0 object-contain" />
          ) : (
            <Box
              size={24}
              className="h-6 w-6 shrink-0 text-cyber-text-secondary"
              aria-hidden="true"
            />
          )}
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <span className="min-w-0 truncate text-sm font-semibold text-cyber-text">
              {displayName}
            </span>
            {keyless && <KeylessModelLabel />}
            {model.accountSource && displayPlan && (
              <span className="shrink-0 text-xs text-cyber-text-secondary">{displayPlan}</span>
            )}
          </div>
        </div>
        <div className="free-model-route-row-details relative pointer-events-none">
          <div className="flex min-w-0 items-center justify-end gap-2">
            {authorizationFailed ? (
              <>
                <span role="status" className="shrink-0 text-xs font-semibold text-cyber-error">
                  {t('accountCenter.authFailed')}
                </span>
                {onReauthorize && (
                  <button
                    type="button"
                    aria-label={`${t('accountCenter.reauthorize')} ${accountProviderName}`}
                    aria-busy={reauthorizing || undefined}
                    disabled={reauthorizing}
                    onClick={(event) => {
                      event.stopPropagation();
                      onReauthorize();
                    }}
                    onPointerDown={(event) => event.stopPropagation()}
                    className="pointer-events-auto relative flex h-5 w-5 shrink-0 items-center justify-center rounded-button border border-cyber-border text-cyber-text transition-colors enabled:hover:bg-cyber-text/10 disabled:text-cyber-text-muted disabled:opacity-50"
                  >
                    <RefreshCw
                      size={11}
                      className={reauthorizing ? 'animate-spin' : ''}
                      aria-hidden="true"
                    />
                  </button>
                )}
              </>
            ) : (
              <span
                className={`truncate text-xs ${active ? 'text-cyber-accent' : 'text-cyber-text-muted'}`}
              >
                {statusLabel}
              </span>
            )}
          </div>
          <span
            className="free-model-route-latency flex w-[72px] shrink-0 items-center justify-center text-center text-xs font-mono tabular-nums text-cyber-text-secondary"
            aria-label={t('freeModels.router.responseLatency')}
          >
            {testing ? (
              <LatencyIndicator />
            ) : testResult?.error === 'timeout' || (testResult?.latencyMs ?? 0) > 1000 ? (
              <span className="text-red-500">{t('freeModels.router.testTimeout')}</span>
            ) : testResult?.latencyMs != null ? (
              <span className={modelLatencyColor(testResult.latencyMs)}>
                {testResult.latencyMs}ms
              </span>
            ) : testResult?.error ? (
              <span className="text-red-500">{t('freeModels.router.testFailed')}</span>
            ) : (
              '—'
            )}
          </span>
          {modelPicker}
        </div>
        <div className="free-model-route-row-actions free-model-list-actions relative shrink-0">
          {onRemove && !overlay && (
            <button
              type="button"
              disabled={pending || saving || dragging}
              onClick={onRemove}
              aria-label={`${t('btn.remove')}: ${displayName} (${priority})`}
              className="flex h-7 items-center justify-center rounded-button text-cyber-text-muted hover:text-red-500"
            >
              <Trash2 size={15} />
            </button>
          )}
          {onEdit && !overlay && (
            <button
              type="button"
              disabled={pending || saving || dragging}
              onClick={onEdit}
              aria-label={`${t('btn.edit')}: ${displayName} (${priority})`}
              className="flex h-7 items-center justify-center rounded-button text-cyber-text-muted hover:text-cyber-text"
            >
              <SquarePen size={15} />
            </button>
          )}
          {onEnabledChange && !overlay && (
            <RoutingToggle
              hideLabel
              label={`${t('freeModels.router.candidateEnabled')}: ${displayName} (${priority})`}
              checked={!paused}
              disabled={authorizationFailed}
              pending={pending || saving || dragging}
              onChange={onEnabledChange}
            />
          )}
        </div>
      </div>
    );
  }

  return (
    <div
      ref={cardRef}
      aria-hidden={overlay || undefined}
      className={`free-model-route-node relative min-h-[80px] rounded-card flex items-center ${hasModelPicker ? 'has-model-picker' : ''} ${active ? 'is-active' : ''} ${paused ? 'is-paused' : ''} ${overlay ? 'is-dragging shadow-2xl' : ''}`}
    >
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 overflow-hidden rounded-[inherit]"
      >
        <span
          className={`free-model-route-priority absolute ${compact ? 'is-compact' : 'right-8'} font-mono font-semibold`}
        >
          {priority}
        </span>
      </div>
      {activator}
      <div className="relative z-10 pointer-events-none flex w-full items-center gap-2.5">
        {iconSrc ? (
          <img src={iconSrc} alt="" className="h-12 w-12 shrink-0 object-contain" />
        ) : (
          <Box size={48} className="shrink-0 text-cyber-text-secondary" aria-hidden="true" />
        )}
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div
            className={
              compact ? 'flex h-5 min-w-0 items-center gap-2' : 'flex min-w-0 items-center gap-1'
            }
          >
            <span
              className={`min-w-0 flex-1 text-sm ${compact ? '!leading-5' : ''} font-semibold text-cyber-text truncate`}
            >
              {compact ? displayName : model.provider}
            </span>
            {compact && keyless && <KeylessModelLabel card />}
            {compact && model.accountSource && displayPlan && (
              <span className="shrink-0 whitespace-nowrap text-xs text-cyber-text-secondary">
                {displayPlan}
              </span>
            )}
          </div>
          {modelPicker}
        </div>
      </div>
    </div>
  );
}

function SortableRouteModel({
  registerNode,
  ...props
}: RouteModelCardProps & {
  registerNode: (id: string, node: HTMLDivElement | null) => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: props.model.id });

  return (
    <div ref={(node) => registerNode(props.model.id, node)} className="min-w-0">
      <div
        ref={setNodeRef}
        style={{
          transform: CSS.Transform.toString(transform),
          transition,
          opacity: isDragging ? 0 : undefined,
        }}
      >
        <RouteModelCard
          {...props}
          compact
          dragging={isDragging}
          dragHandleProps={{ ref: setActivatorNodeRef, ...attributes, ...listeners }}
        />
      </div>
    </div>
  );
}

function RouterRemoveDropZone({
  visible,
  stageRef,
}: {
  visible: boolean;
  stageRef: RefObject<HTMLDivElement>;
}) {
  const { t } = useI18n();
  const { setNodeRef, isOver } = useDroppable({
    id: ROUTER_REMOVE_DROP_ID,
    disabled: !visible,
  });
  const { measureDroppableContainers } = useDndContext();
  const [position, setPosition] = useState({ left: 0, width: 0, bottom: 12 });

  useLayoutEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const update = () => {
      const rect = stage.getBoundingClientRect();
      const next = {
        left: rect.left + NODE_SIDE_PADDING,
        width: Math.max(0, rect.width - NODE_SIDE_PADDING * 2),
        bottom: Math.max(12, window.innerHeight - rect.bottom + 12),
      };
      setPosition((current) =>
        current.left === next.left && current.width === next.width && current.bottom === next.bottom
          ? current
          : next
      );
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(stage);
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
    };
  }, [stageRef, visible]);

  useLayoutEffect(() => {
    if (visible && position.width > 0) measureDroppableContainers([ROUTER_REMOVE_DROP_ID]);
  }, [visible, position, measureDroppableContainers]);

  return (
    <div
      ref={setNodeRef}
      id={ROUTER_REMOVE_DROP_ID}
      aria-hidden={!visible}
      className="pointer-events-none fixed z-[1001] h-16"
      style={position}
    >
      <div
        className={`free-model-remove-zone flex h-full items-center justify-center gap-3 rounded-xl px-4 font-sans text-sm ${visible ? 'is-visible' : ''} ${visible && isOver ? 'is-over' : ''}`}
        role="status"
        aria-live="polite"
      >
        <Trash2 size={24} aria-hidden="true" />
        <span>
          {t(isOver ? 'freeModels.router.releaseToRemove' : 'freeModels.router.dropToRemove')}
        </span>
      </div>
    </div>
  );
}

type RouteCatalogInput = modelApi.UpstreamModelCatalogInput | { accountSource: AccountSource };

type RouteModelCatalog = modelApi.UpstreamModelCatalog & {
  account?: ProxyAccount;
  accountCacheSnapshot?: string;
};

function routeCatalogBinding(model: RouteModelNode) {
  if (model.id === 'local-server') return null;
  if (model.accountSource) {
    const { provider, accountId } = model.accountSource;
    const input: RouteCatalogInput = { accountSource: { provider, accountId } };
    return { input, key: JSON.stringify(input) };
  }
  if (model.apiKey === undefined) return null;
  const input: RouteCatalogInput = {
    baseUrl: model.baseUrl.trim(),
    anthropicUrl: model.anthropicUrl?.trim() ?? '',
    apiKey: model.apiKey.trim(),
  };
  return { input, key: JSON.stringify(input) };
}

export function FreeModelsMain() {
  const { activeRouterId, layout } = useFreeModels();
  const routerFocusRef = useRef(false);
  const routerTabPositionRef = useRef<ViewModeTabPosition | null>(null);
  return (
    <>
      <FreeModelsContent
        key={activeRouterId}
        routerFocusRef={routerFocusRef}
        routerTabPositionRef={routerTabPositionRef}
      />
      {layout === 'terminal' && <RouterTerminal />}
    </>
  );
}

function FreeModelsContent({
  routerFocusRef,
  routerTabPositionRef,
}: {
  routerFocusRef: { current: boolean };
  routerTabPositionRef: { current: ViewModeTabPosition | null };
}) {
  const activePage = useNavigationStore((state) => state.activePage);
  const appManager = useContext(AppManagerContext);
  const { t } = useI18n();
  const docVisible = useDocumentVisible();
  const { showToast } = useToast();
  const confirm = useConfirm();
  const { updateUserModel, handleCardEdit } = useModelNexus();
  const {
    layout,
    activeDragId,
    setActiveDragId,
    models: freeModels,
    customModels,
    proxyAccounts,
    selectedIds,
    activeRouterEnabled: routerEnabled,
    activeRouterId,
    groupPending,
    removeSelectedModel,
    reorderSelectedModel,
    changeSelectedModel,
    savingModelIds,
    pausedIds,
    candidateTogglePending,
    setCandidateEnabled,
  } = useFreeModels();
  const [removingIds, setRemovingIds] = useState<Set<string>>(() => new Set());
  const removalRequests = useRef(new Set<string>());
  const autoToggleRef = useRef(new Map<string, 'pause' | 'resume'>());
  const removeFromList = async (id: string) => {
    if (removalRequests.current.has(id)) return;
    removalRequests.current.add(id);
    setRemovingIds(new Set(removalRequests.current));
    try {
      const accepted = await confirm({
        title: t('freeModels.router.removeTitle'),
        message: t('freeModels.router.removeConfirm'),
        confirmText: t('btn.remove'),
        cancelText: t('btn.cancel'),
        type: 'danger',
      });
      if (accepted) await removeSelectedModel(id);
    } finally {
      removalRequests.current.delete(id);
      setRemovingIds(new Set(removalRequests.current));
    }
  };
  const [routeCatalogs, setRouteCatalogs] = useState<Map<string, RouteModelCatalog>>(
    () => new Map()
  );
  const [refreshingCatalogs, setRefreshingCatalogs] = useState<Set<string>>(() => new Set());
  const catalogGeneration = useRef(0);
  const catalogRequests = useRef(new Map<string, object>());
  const catalogRevisions = useRef(new Map<string, number>());
  const loadedCatalogs = useRef(new Set<string>());
  const catalogBindingsKey = JSON.stringify(
    [
      ...new Set(
        customModels.flatMap((model) => {
          const binding = routeCatalogBinding(model);
          return binding ? [binding.key] : [];
        })
      ),
    ].sort()
  );
  const currentCatalogBindings = useRef(new Set<string>());

  useEffect(() => {
    const generation = ++catalogGeneration.current;
    const keys: string[] = JSON.parse(catalogBindingsKey);
    currentCatalogBindings.current = new Set(keys);
    const requests = catalogRequests.current;
    keys.forEach((key) => {
      const revision = catalogRevisions.current.get(key);
      const input: RouteCatalogInput = JSON.parse(key);
      const models =
        'accountSource' in input ? getCachedAccountProxyModels(input.accountSource) : undefined;
      const cached =
        'accountSource' in input
          ? Promise.resolve(
              models === undefined
                ? null
                : {
                    models: models.map((id) => ({ id })),
                    requiresApiKey: false,
                    accountCacheSnapshot: JSON.stringify(models),
                  }
            )
          : modelApi.getCachedUpstreamModelCatalog(input);
      void cached
        .then((catalog) => {
          if (
            catalog &&
            catalogGeneration.current === generation &&
            catalogRevisions.current.get(key) === revision
          ) {
            setRouteCatalogs((current) =>
              'accountSource' in input && current.has(key)
                ? current
                : new Map(current).set(key, catalog)
            );
          }
        })
        .catch(() => undefined);
    });
    return () => {
      catalogGeneration.current = generation + 1;
      requests.clear();
      setRefreshingCatalogs(new Set());
    };
  }, [catalogBindingsKey]);

  const refreshRouteCatalog = async (model: RouteModelNode) => {
    const binding = routeCatalogBinding(model);
    if (
      !binding ||
      !currentCatalogBindings.current.has(binding.key) ||
      catalogRequests.current.has(binding.key) ||
      loadedCatalogs.current.has(binding.key)
    )
      return;
    const { key, input } = binding;
    const generation = catalogGeneration.current;
    const request = {};
    catalogRequests.current.set(key, request);
    catalogRevisions.current.set(key, (catalogRevisions.current.get(key) ?? 0) + 1);
    setRefreshingCatalogs((current) => new Set(current).add(key));
    const isCurrent = () =>
      catalogGeneration.current === generation && currentCatalogBindings.current.has(key);
    try {
      let catalog: RouteModelCatalog;
      if ('accountSource' in input) {
        const source = input.accountSource;
        const models = await getAccountProxyModels(source);
        if (!isCurrent()) return;
        let account: ProxyAccount | undefined;
        try {
          const accounts = await listProxyAccounts(source.provider);
          account = accounts.find(
            (entry) =>
              entry.source.provider === source.provider &&
              entry.source.accountId === source.accountId
          );
        } catch (error) {
          console.error('Load proxy model labels failed:', error);
        }
        if (!isCurrent()) return;
        cacheAccountProxyModels(source, models);
        catalog = {
          models: models.map((id) => ({ id })),
          requiresApiKey: false,
          account,
          accountCacheSnapshot: JSON.stringify(getCachedAccountProxyModels(source) ?? null),
        };
      } else {
        catalog = await modelApi.fetchUpstreamModelCatalog(input);
      }
      if (isCurrent()) {
        loadedCatalogs.current.add(key);
        setRouteCatalogs((current) => new Map(current).set(key, catalog));
      }
    } catch {
      // Keep cached choices on an automatic load failure; reopening can retry.
    } finally {
      if (catalogRequests.current.get(key) === request) {
        catalogRequests.current.delete(key);
        if (isCurrent())
          setRefreshingCatalogs((current) => {
            const next = new Set(current);
            next.delete(key);
            return next;
          });
      }
    }
  };
  const routeCatalogFor = (model: RouteModelNode) => {
    const binding = routeCatalogBinding(model);
    return binding ? routeCatalogs.get(binding.key) : undefined;
  };
  const modelMetadataFor = (model: RouteModelNode) => {
    const binding = routeCatalogBinding(model);
    const refresh = {
      refreshing: binding ? refreshingCatalogs.has(binding.key) : false,
      onLoadModels: binding ? () => void refreshRouteCatalog(model) : undefined,
    };
    if (model.accountSource || model.id === 'local-server') return refresh;
    const upstream = routeCatalogFor(model)?.models ?? [];
    const paidModelIds = upstream
      .filter(
        (entry) => entry.free !== true && (entry.free === false || (entry.multiplier ?? 0) > 0)
      )
      .map((entry) => entry.id);
    const freeModelIds = [
      ...new Set([
        ...freeModels
          .filter(
            (entry) =>
              entry.baseUrl === model.baseUrl &&
              (entry.freeType === 'perpetual' || isKeylessFreeModel(entry)) &&
              !paidModelIds.includes(entry.modelId)
          )
          .map((entry) => entry.modelId),
        ...upstream.filter((entry) => entry.free === true).map((entry) => entry.id),
      ]),
    ];
    const optionMultipliers = Object.fromEntries(
      upstream.flatMap((entry) =>
        entry.multiplier === undefined ? [] : [[entry.id, entry.multiplier]]
      )
    );
    return {
      freeModelIds,
      paidModelIds,
      optionMultipliers,
      ...refresh,
    };
  };
  const accountFor = (model: RouteModelNode) => {
    const source = model.accountSource;
    return source
      ? (routeCatalogFor(model)?.account ??
          proxyAccounts[source.provider]?.find(
            (account) =>
              account.source.provider === source.provider &&
              account.source.accountId === source.accountId
          ))
      : undefined;
  };
  const keylessFor = (model: RouteModelNode) =>
    !model.accountSource &&
    model.id !== 'local-server' &&
    freeModels.some((entry) => entry.baseUrl === model.baseUrl && isKeylessFreeModel(entry));
  const providerIconFor = (model: RouteModelNode) => {
    if (model.accountSource || model.id === 'local-server' || model.scope !== 'smartRouter')
      return undefined;
    const provider = freeModels.find((entry) => entry.baseUrl === model.baseUrl);
    return provider ? getModelIcon(provider.provider, '') : undefined;
  };
  const modelIdOptionsFor = (model: RouteModelNode) => {
    if (model.id === 'local-server') return [];
    if (model.accountSource) {
      const cached = getCachedAccountProxyModels(model.accountSource);
      const catalog = routeCatalogFor(model);
      const fetchedModels = catalog?.models.map((entry) => entry.id);
      // Keep a successful refresh usable if persistence fails, while observing
      // a later directory update from the account add/edit dialog.
      const models =
        (catalog?.accountCacheSnapshot === JSON.stringify(cached ?? null)
          ? fetchedModels
          : cached) ??
        fetchedModels ??
        [];
      return [
        ...new Set([
          model.modelId,
          ...accountModelPickerOptions(models, model.modelId, accountFor(model)?.modelLabels),
        ]),
      ].filter(Boolean);
    }
    const catalog = routeCatalogFor(model);
    return [
      ...new Set([
        model.modelId,
        ...(catalog
          ? catalog.models.map((entry) => entry.id)
          : freeModels
              .filter((entry) => entry.baseUrl === model.baseUrl)
              .map((entry) => entry.modelId)),
      ]),
    ].filter(Boolean);
  };
  const switchModel = async (id: string, modelId: string) => {
    const model = customModels.find((entry) => entry.id === id);
    if (
      !model ||
      model.modelId === modelId ||
      savingModelIds.has(id) ||
      !modelIdOptionsFor(model).includes(modelId)
    )
      return;
    try {
      const updated = await changeSelectedModel(id, modelId);
      if (!updated) return;
      updateUserModel(updated, {
        name: updated.name,
        baseUrl: updated.baseUrl,
        anthropicUrl: updated.anthropicUrl ?? '',
        apiKey: updated.apiKey,
        modelId: updated.modelId ?? modelId,
        accountSource: updated.accountSource,
      });
    } catch (error) {
      console.error('Change smart router model failed:', error);
      showToast('error', t('error.requestFailed'));
    }
  };
  const selectedModels = useMemo(() => {
    const modelsById = new Map(customModels.map((model) => [model.id, model]));
    return [...selectedIds].flatMap((id) => {
      const model = modelsById.get(id);
      return model ? [model] : [];
    });
  }, [customModels, selectedIds]);
  const responseTests = useRouteResponseTests(
    selectedModels,
    activePage === 'freeModels' && layout === 'list',
    activeRouterId
  );
  const accountStatusByModel = useMemo(() => {
    const bindings = new Map<
      string,
      { status: ReturnType<typeof accountProxyStatus>; accountId: string }
    >();
    if (!appManager) return bindings;
    for (const model of selectedModels) {
      const source = model.accountSource;
      if (!source) continue;
      const status = accountProxyStatus(appManager, source.provider);
      bindings.set(model.id, { status, accountId: source.accountId });
    }
    return bindings;
  }, [appManager, selectedModels]);
  const authorizationByModel = useMemo(() => {
    const failed = new Map<string, ReturnType<typeof accountProxyStatus>>();
    for (const [id, binding] of accountStatusByModel) {
      if (binding.status.authorizationFailedIds.has(binding.accountId))
        failed.set(id, binding.status);
    }
    return failed;
  }, [accountStatusByModel]);
  useEffect(() => {
    for (const id of authorizationByModel.keys()) {
      if (!autoToggleRef.current.has(id) && !pausedIds.has(id) && !candidateTogglePending.has(id)) {
        autoToggleRef.current.set(id, 'pause');
        void setCandidateEnabled(id, false);
      }
    }
    for (const [id, intent] of [...autoToggleRef.current]) {
      if (authorizationByModel.has(id)) continue;
      const binding = accountStatusByModel.get(id);
      const accountExists =
        binding != null &&
        binding.status.accounts.some((account) => account.id === binding.accountId);
      if (!pausedIds.has(id)) {
        autoToggleRef.current.delete(id);
      } else if (accountExists && intent === 'pause' && !candidateTogglePending.has(id)) {
        autoToggleRef.current.set(id, 'resume');
        void setCandidateEnabled(id, true);
      } else if (!accountExists) {
        autoToggleRef.current.delete(id);
      }
    }
  }, [
    accountStatusByModel,
    authorizationByModel,
    pausedIds,
    candidateTogglePending,
    setCandidateEnabled,
  ]);
  useEffect(() => () => setActiveDragId(null), [setActiveDragId, activePage]);
  const activeDragModel = selectedModels.find((model) => model.id === activeDragId);
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 5 },
    }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );
  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    setActiveDragId(null);
    if (layout === 'cards' && over?.id === ROUTER_REMOVE_DROP_ID) {
      if (activeDragId === String(active.id)) void removeSelectedModel(String(active.id));
      return;
    }
    if (over && active.id !== over.id) {
      void reorderSelectedModel(String(active.id), String(over.id));
    }
  };
  const stageRef = useRef<HTMLDivElement | null>(null);
  const hubRef = useRef<HTMLDivElement | null>(null);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());
  const [routePaths, setRoutePaths] = useState<RoutePath[]>([]);
  const [activityPaths, setActivityPaths] = useState<RoutePath[]>([]);
  const [routeArrow, setRouteArrow] = useState<RouteArrow | null>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 1, height: 1 });
  const [nodeColumns, setNodeColumns] = useState(3);
  const [savedActivity, setRouterActivity] = useState<
    api.SmartRouterActivity & { routerId?: string }
  >({
    candidateId: null,
    active: false,
    sequence: 0,
    updatedAtMs: 0,
  });
  const routerActivity =
    savedActivity.routerId === activeRouterId
      ? savedActivity
      : { candidateId: null, active: false, sequence: 0, updatedAtMs: 0 };
  const [activityObservedAtMs, setActivityObservedAtMs] = useState(0);
  const showRouteActivity = layout !== 'terminal';

  useEffect(() => {
    let cancelled = false;
    if (!showRouteActivity || !routerEnabled || !docVisible || activePage !== 'freeModels') return;
    let pending = false;
    let errorReported = false;
    const pollActivity = async () => {
      if (pending) return;
      pending = true;
      try {
        const activity = await api.getSmartRouterActivity(
          ...(activeRouterId === 'smart-router' ? [] : [activeRouterId])
        );
        if (!cancelled) {
          setRouterActivity({ ...activity, routerId: activeRouterId });
          setActivityObservedAtMs(Date.now());
        }
      } catch (error) {
        if (!cancelled && !errorReported) {
          errorReported = true;
          console.error('Load smart router activity failed:', error);
        }
      } finally {
        pending = false;
      }
    };
    void pollActivity();
    const timer = window.setInterval(() => void pollActivity(), ROUTER_ACTIVITY_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [showRouteActivity, routerEnabled, docVisible, activePage, activeRouterId]);

  const setNodeRef = useCallback((id: string, node: HTMLDivElement | null) => {
    if (node) nodeRefs.current.set(id, node);
    else nodeRefs.current.delete(id);
  }, []);

  const updatePaths = useCallback(() => {
    const stage = stageRef.current;
    const hub = hubRef.current;
    if (!stage || !hub) return;
    const stageBox = stage.getBoundingClientRect();
    const hubBox = hub.getBoundingClientRect();
    setNodeColumns(
      Math.max(
        1,
        Math.floor(
          (stageBox.width - NODE_SIDE_PADDING * 2 + NODE_COLUMN_GAP) /
            (NODE_MIN_WIDTH + NODE_COLUMN_GAP)
        )
      )
    );
    const endX = stageBox.width / 2;
    const endY = hubBox.bottom - stageBox.top;

    const allEntries = [...nodeRefs.current].map(([id, node]) => {
      const box = node.getBoundingClientRect();
      return {
        id,
        x: box.left - stageBox.left + box.width / 2,
        top: box.top - stageBox.top,
        bottom: box.bottom - stageBox.top,
      };
    });
    const lastEnabledTopByColumn = new Map<number, number>();
    for (const entry of allEntries) {
      if (pausedIds.has(entry.id)) continue;
      const column = Math.round(entry.x);
      lastEnabledTopByColumn.set(
        column,
        Math.max(lastEnabledTopByColumn.get(column) ?? entry.top, entry.top)
      );
    }
    const entries = allEntries.filter(
      (entry) => entry.top <= (lastEnabledTopByColumn.get(Math.round(entry.x)) ?? -Infinity)
    );
    entries.sort((left, right) => left.top - right.top || left.x - right.x);

    const rows: (typeof entries)[] = [];
    for (const entry of entries) {
      const row = rows.find((candidate) => Math.abs(candidate[0].top - entry.top) < 12);
      if (row) row.push(entry);
      else rows.push([entry]);
    }

    const nextPaths: RoutePath[] = [];
    const nextActivityPaths: RoutePath[] = [];
    const parentById = new Map<string, (typeof entries)[number]>();
    const firstRow = rows[0];
    const directToHub = firstRow?.length === 1 && Math.abs(firstRow[0].x - endX) < 1;
    let nextArrow: RouteArrow | null = null;
    let routeBusY: number | null = null;
    let routeLineEndY: number | null = null;
    if (firstRow) {
      const arrowY = endY + HUB_ARROW_GAP;
      const lineEndY = arrowY + HUB_ARROW_HEIGHT + HUB_ARROW_LINE_GAP;
      routeLineEndY = lineEndY;
      if (directToHub) {
        const entry = firstRow[0];
        nextPaths.push({
          id: entry.id,
          d: `M ${entry.x} ${entry.top - 3} V ${lineEndY}`,
        });
        nextArrow = { x: endX, y: arrowY };
      } else {
        const busY = endY + (firstRow[0].top - endY) / 2;
        routeBusY = busY;
        nextPaths.push({
          id: 'hub-trunk',
          d: `M ${endX} ${busY} V ${lineEndY}`,
        });
        nextArrow = { x: endX, y: arrowY };
        const busJoins = firstRow.map((entry) => {
          const top = entry.top - 3;
          const dx = endX - entry.x;
          const radius = Math.min(12, (top - busY) / 2, Math.abs(dx) / 2);
          const joinX = Math.abs(dx) < 1 ? entry.x : entry.x + Math.sign(dx) * radius;
          nextPaths.push({
            id: entry.id,
            d:
              Math.abs(dx) < 1
                ? `M ${entry.x} ${top} V ${busY}`
                : `M ${entry.x} ${top} V ${busY + radius} A ${radius} ${radius} 0 0 ${dx > 0 ? 1 : 0} ${joinX} ${busY}`,
          });
          return joinX;
        });
        const leftJoin = Math.min(endX, ...busJoins);
        const rightJoin = Math.max(endX, ...busJoins);
        if (leftJoin < endX)
          nextPaths.push({ id: 'hub-bus-left', d: `M ${leftJoin} ${busY} H ${endX}` });
        if (rightJoin > endX)
          nextPaths.push({ id: 'hub-bus-right', d: `M ${rightJoin} ${busY} H ${endX}` });
      }
    }

    rows.slice(1).forEach((row, rowIndex) => {
      const parentRow = rows[rowIndex];
      row.forEach((entry) => {
        const parent = parentRow.reduce((closest, candidate) =>
          Math.abs(candidate.x - entry.x) < Math.abs(closest.x - entry.x) ? candidate : closest
        );
        const bridgeY = parent.bottom + (entry.top - parent.bottom) / 2;
        parentById.set(entry.id, parent);
        nextPaths.push({
          id: entry.id,
          d:
            Math.abs(entry.x - parent.x) < 1
              ? `M ${entry.x} ${entry.top - 3} V ${parent.bottom + 3}`
              : `M ${entry.x} ${entry.top - 3} V ${bridgeY} H ${parent.x} V ${parent.bottom + 3}`,
        });
      });
    });

    if (firstRow && routeLineEndY !== null) {
      entries.forEach((entry) => {
        if (pausedIds.has(entry.id)) return;
        let current = entry;
        let d = `M ${entry.x} ${entry.top - 3}`;
        let parent = parentById.get(current.id);
        while (parent) {
          const bridgeY = parent.bottom + (current.top - parent.bottom) / 2;
          d +=
            Math.abs(current.x - parent.x) < 1
              ? ` V ${parent.bottom + 3}`
              : ` V ${bridgeY} H ${parent.x} V ${parent.bottom + 3}`;
          d += ` V ${parent.top - 3}`;
          current = parent;
          parent = parentById.get(current.id);
        }
        if (directToHub) {
          d += ` V ${routeLineEndY}`;
        } else if (routeBusY !== null) {
          d += ` ${routeBranchToBus(current.x, current.top - 3, routeBusY, endX)} V ${routeLineEndY}`;
        }
        nextActivityPaths.push({ id: entry.id, d });
      });
    }
    setCanvasSize({ width: stageBox.width, height: stageBox.height });
    setRoutePaths(nextPaths);
    setActivityPaths(nextActivityPaths);
    setRouteArrow(nextArrow);
  }, [pausedIds]);

  useLayoutEffect(() => {
    if (layout !== 'cards' || !docVisible || activePage !== 'freeModels') return;
    const frame = requestAnimationFrame(updatePaths);
    const stage = stageRef.current;
    if (!stage) return () => cancelAnimationFrame(frame);
    const observer = new ResizeObserver(updatePaths);
    observer.observe(stage);
    if (hubRef.current) observer.observe(hubRef.current);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [selectedModels, nodeColumns, updatePaths, layout, docVisible, activePage]);

  const activityIsVisible =
    routerActivity.candidateId !== null &&
    (routerActivity.active ||
      activityObservedAtMs - routerActivity.updatedAtMs < ROUTER_ACTIVITY_GRACE_MS);
  const activeRoutePath =
    activityIsVisible && !pausedIds.has(routerActivity.candidateId ?? '')
      ? activityPaths.find((path) => path.id === routerActivity.candidateId)
      : undefined;
  const workingIds = routerActivity.active
    ? (routerActivity.activeCandidateIds ??
      (routerActivity.candidateId ? [routerActivity.candidateId] : []))
    : [];
  const routeRowCount = Math.ceil(selectedModels.length / nodeColumns);
  const stageMinHeight = Math.max(
    550,
    196 +
      HUB_TO_NODE_GAP +
      routeRowCount * 80 +
      Math.max(0, routeRowCount - 1) * NODE_ROW_GAP +
      56 +
      24
  );

  if (layout === 'terminal') return null;

  return (
    <div
      className={`free-model-router smart-router flex ${layout === 'list' ? 'min-h-full' : 'h-full'} min-h-[620px] flex-col pb-1`}
    >
      <div
        aria-busy={groupPending || undefined}
        ref={stageRef}
        className={`relative ${layout === 'list' ? 'grow shrink-0' : 'overflow-hidden flex-1 shrink-0'} ${routerEnabled ? '' : 'is-disabled'}`}
        style={{ minHeight: layout === 'cards' ? stageMinHeight : 550 }}
      >
        {layout === 'cards' && (
          <svg
            className="absolute inset-0 z-0 h-full w-full pointer-events-none"
            viewBox={`0 0 ${canvasSize.width} ${canvasSize.height}`}
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            {routePaths.map((path) => (
              <path key={path.id} d={path.d} className="free-model-route-path" />
            ))}
            {routerEnabled && activeRoutePath && (
              <path
                key={activeRoutePath.id}
                d={activeRoutePath.d}
                pathLength="1"
                ref={(path) => {
                  if (path)
                    path.style.setProperty(
                      '--free-model-route-glow-span',
                      String(Math.min(1, Math.max(0.07, 20 / path.getTotalLength())))
                    );
                }}
                className="free-model-route-glow"
              />
            )}
            {routeArrow && (
              <path
                d={`M ${routeArrow.x} ${routeArrow.y} L ${routeArrow.x - 6} ${routeArrow.y + HUB_ARROW_HEIGHT} H ${routeArrow.x + 6} Z`}
                className="free-model-route-arrow"
              />
            )}
          </svg>
        )}

        <RouterGroups
          key={activePage}
          hubRef={hubRef}
          restoreFocusRef={routerFocusRef}
          tabPositionRef={routerTabPositionRef}
          refreshAction={
            layout === 'list' ? (
              <button
                type="button"
                onClick={() => void responseTests.test()}
                disabled={
                  responseTests.isTesting || selectedModels.length === 0 || savingModelIds.size > 0
                }
                aria-label={t('btn.pingAll')}
                aria-busy={responseTests.isTesting || undefined}
                className="free-model-router-list-button relative rounded-button px-3 font-mono text-xs text-cyber-text-muted disabled:opacity-50"
              >
                <span className={`relative top-px${responseTests.isTesting ? ' invisible' : ''}`}>
                  {t('btn.pingAll')}
                </span>
                {responseTests.isTesting && (
                  <RefreshCw
                    size={12}
                    className="absolute inset-y-0 right-3 my-auto animate-spin"
                    aria-hidden
                  />
                )}
              </button>
            ) : undefined
          }
        />

        {selectedModels.length === 0 ? (
          <div
            className={`absolute inset-x-0 bottom-0 ${layout === 'cards' ? 'top-[196px]' : 'top-24'} z-10 flex items-center justify-center text-center pointer-events-none`}
          >
            <div>
              <div className="font-medium text-sm text-cyber-text">
                {t('freeModels.router.emptyTitle')}
              </div>
              <div className="mt-2 text-xs text-cyber-text-muted">
                {t('freeModels.router.emptyDesc')}
              </div>
            </div>
          </div>
        ) : (
          <>
            <DndContext
              key={activePage === 'freeModels' ? 'active' : 'hidden'}
              sensors={sensors}
              modifiers={layout === 'list' ? [restrictRouteToVerticalAxis] : undefined}
              collisionDetection={routeCollisionDetection}
              onDragStart={({ active }) => setActiveDragId(String(active.id))}
              onDragEnd={handleDragEnd}
              onDragCancel={() => setActiveDragId(null)}
            >
              <SortableContext
                items={selectedModels.map((model) => model.id)}
                strategy={layout === 'list' ? verticalListSortingStrategy : rectSortingStrategy}
              >
                <div
                  className={`${layout === 'list' ? 'free-model-route-list' : 'free-model-node-grid'} relative z-10`}
                  style={
                    layout === 'cards'
                      ? {
                          gridTemplateColumns: `repeat(${Math.min(selectedModels.length, nodeColumns)}, minmax(0, calc((100% - ${(nodeColumns - 1) * NODE_COLUMN_GAP}px) / ${nodeColumns})))`,
                          columnGap: NODE_COLUMN_GAP,
                          rowGap: NODE_ROW_GAP,
                          paddingInline: NODE_SIDE_PADDING,
                          marginTop: HUB_TO_NODE_GAP,
                        }
                      : { paddingInline: NODE_SIDE_PADDING }
                  }
                >
                  {selectedModels.map((model, index) => {
                    const authorization = authorizationByModel.get(model.id);
                    const authorizationFailed = Boolean(authorization);
                    return (
                      <SortableRouteModel
                        key={model.id}
                        model={model}
                        account={accountFor(model)}
                        keyless={keylessFor(model)}
                        providerIconSrc={providerIconFor(model)}
                        priority={index + 1}
                        list={layout === 'list'}
                        testing={responseTests.pendingIds.has(model.id)}
                        testResult={responseTests.results[model.id]}
                        paused={pausedIds.has(model.id) || authorizationFailed}
                        pending={candidateTogglePending.has(model.id) || removingIds.has(model.id)}
                        active={
                          routerEnabled &&
                          !authorizationFailed &&
                          !pausedIds.has(model.id) &&
                          (layout === 'list'
                            ? workingIds.includes(model.id)
                            : activeRoutePath?.id === model.id)
                        }
                        statusLabel={t(
                          authorizationFailed
                            ? 'accountCenter.authFailed'
                            : pausedIds.has(model.id)
                              ? 'freeModels.router.paused'
                              : !routerEnabled
                                ? 'freeModels.router.disabled'
                                : workingIds.includes(model.id)
                                  ? 'freeModels.router.working'
                                  : 'freeModels.router.idle'
                        )}
                        authorizationFailed={authorizationFailed}
                        reauthorizing={authorization?.busy}
                        onReauthorize={authorization ? () => void authorization.add() : undefined}
                        onEnabledChange={(enabled) => void setCandidateEnabled(model.id, enabled)}
                        onRemove={() => void removeFromList(model.id)}
                        onEdit={
                          model.id === 'local-server'
                            ? undefined
                            : () =>
                                void handleCardEdit({
                                  internalId: model.internalId,
                                  name: model.provider,
                                  baseUrl: model.baseUrl,
                                  anthropicUrl: model.anthropicUrl,
                                  apiKey: model.apiKey ?? '',
                                  modelId: model.modelId,
                                  accountSource: model.accountSource,
                                  scope: model.scope,
                                })
                        }
                        modelIdOptions={modelIdOptionsFor(model)}
                        {...modelMetadataFor(model)}
                        saving={savingModelIds.has(model.id)}
                        onModelChange={switchModel}
                        registerNode={setNodeRef}
                      />
                    );
                  })}
                </div>
              </SortableContext>
              {layout === 'cards' &&
                createPortal(
                  <RouterRemoveDropZone visible={activeDragId !== null} stageRef={stageRef} />,
                  document.body
                )}
              {createPortal(
                <DragOverlay
                  dropAnimation={null}
                  zIndex={1002}
                  className={`pointer-events-none font-sans text-cyber-text ${layout === 'list' ? 'smart-router' : ''}`}
                >
                  {activeDragModel && (
                    <RouteModelCard
                      model={activeDragModel}
                      account={accountFor(activeDragModel)}
                      keyless={keylessFor(activeDragModel)}
                      providerIconSrc={providerIconFor(activeDragModel)}
                      compact
                      list={layout === 'list'}
                      paused={
                        pausedIds.has(activeDragModel.id) ||
                        authorizationByModel.has(activeDragModel.id)
                      }
                      priority={selectedModels.indexOf(activeDragModel) + 1}
                      overlay
                    />
                  )}
                </DragOverlay>,
                document.body
              )}
            </DndContext>
            <p className="mx-auto mt-14 flex max-w-[680px] flex-wrap items-center justify-center gap-x-2 gap-y-1 cursor-default text-center text-sm leading-6 text-cyber-text-secondary">
              <span>
                {t(
                  layout === 'list'
                    ? 'freeModels.router.listReorderHint'
                    : 'freeModels.router.reorderHint'
                )}
              </span>
              {layout === 'cards' && (
                <>
                  <span aria-hidden="true" className="text-cyber-text-muted">
                    |
                  </span>
                  <span className="inline-flex items-center gap-1.5">
                    <span>{t('freeModels.router.removeHintPrefix')}</span>
                    <Trash2 size={14} role="img" aria-label={t('btn.delete')} />
                    <span>{t('freeModels.router.removeHintSuffix')}</span>
                  </span>
                </>
              )}
            </p>
          </>
        )}
      </div>
    </div>
  );
}

function KeylessModelLabel({ card = false }: { card?: boolean }) {
  const { t } = useI18n();
  return (
    <span
      className={`shrink-0 whitespace-nowrap font-medium text-cyber-accent ${card ? 'text-xs !leading-5' : 'text-[10px] leading-none'}`}
    >
      {t('freeModels.noApiKey')}
    </span>
  );
}

function FreeModelProviderRow({
  group,
  selected,
  onAdd,
}: {
  group: FreeModelProviderGroup;
  selected: boolean;
  onAdd: () => void;
}) {
  const { t } = useI18n();
  const iconSrc = getModelIcon(group.name, '');
  const keyless = group.models.every(isKeylessFreeModel);
  const hostname = (() => {
    try {
      return new URL(group.baseUrl).hostname;
    } catch {
      return group.baseUrl;
    }
  })();
  const websiteUrl = group.models[0]?.websiteUrl || group.docsUrl;
  const openDocs = () => shellOpen(websiteUrl).catch(() => window.open(websiteUrl, '_blank'));

  return (
    <div
      className={`relative flex h-12 shrink-0 items-stretch rounded-card overflow-hidden border-2 ${selected ? 'border-cyber-accent/70 bg-cyber-accent/10' : 'border-transparent bg-cyber-surface/60'}`}
    >
      <button
        type="button"
        onClick={onAdd}
        aria-label={`${t('freeModels.addToRouter')}: ${group.name}`}
        aria-pressed={selected}
        className="group/left flex-1 min-h-0 bg-gradient-to-r from-transparent to-transparent hover:from-cyber-text/15 hover:to-transparent transition-[background-image] duration-200"
      />
      <button
        type="button"
        onClick={openDocs}
        aria-label={`${t('freeModels.docs')}: ${group.name}`}
        className="group/right flex-1 min-h-0 bg-gradient-to-l from-transparent to-transparent hover:from-cyber-text/15 hover:to-transparent transition-[background-image] duration-200"
      />

      <div className="pointer-events-none absolute inset-0 flex items-center gap-3 px-3">
        {selected ? (
          <Check
            size={22}
            strokeWidth={2.5}
            className="flex-shrink-0 text-cyber-accent group-hover/left:scale-110 transition-all"
          />
        ) : (
          <Plus
            size={22}
            strokeWidth={2.5}
            className="flex-shrink-0 text-cyber-text-muted group-hover/left:text-cyber-text group-hover/left:scale-110 transition-all"
          />
        )}
        <div className="flex-shrink-0">
          {iconSrc ? (
            <img
              src={iconSrc}
              alt=""
              className="w-6 h-6"
              onError={(event) => {
                (event.target as HTMLImageElement).style.display = 'none';
              }}
            />
          ) : (
            <div className="w-6 h-6 flex items-center justify-center text-cyber-text">
              <Box size={22} />
            </div>
          )}
        </div>
        <div className="flex-1 min-w-0 grid grid-cols-[minmax(0,1fr)] auto-rows-[16px] items-center">
          <div className="flex min-w-0 items-center gap-2 text-sm font-bold leading-none">
            <span className="min-w-0 truncate">{group.name}</span>
            {keyless && <KeylessModelLabel />}
          </div>
          <div className="text-[10px] text-cyber-text-secondary truncate leading-tight opacity-70">
            {hostname}
          </div>
        </div>
        <ExternalLink
          size={18}
          strokeWidth={2.25}
          className="flex-shrink-0 text-cyber-text-muted group-hover/right:text-cyber-text group-hover/right:scale-110 transition-all"
        />
      </div>
    </div>
  );
}

export function FreeModelsPanel() {
  const { t, locale } = useI18n();
  const compactTabs = locale === 'en' || locale === 'ja';
  const { showToast } = useToast();
  const confirm = useConfirm();
  const [activeTab, setActiveTab] = useState<'saved' | 'account' | 'free'>('saved');
  const { acknowledged: accountRiskAcknowledged } = useAccountAccessRisk();
  const [addingId, setAddingId] = useState<string | null>(null);
  const setActivePage = useNavigationStore((state) => state.setActivePage);
  const modelContext = useModelNexus();
  const {
    models,
    customModels,
    selectedIds,
    refreshing,
    scanProvider,
    scanProgress,
    refresh,
    addSelectedModel,
    removeSelectedModel,
  } = useFreeModels();
  const {
    userModels,
    isLoadingModels,
    modelUsageData,
    refreshingUsageIds,
    isRefreshingUsage,
    refreshSingleUsage,
    volcAkSkMissingIds,
    openAkskModal,
    handleCardEdit,
    handleCardDelete,
    setNewModelForm,
    setEditingModelId,
    setModelModalDestination,
    setShowAddModelModal,
    setKeyDestroyed,
    setShowApiKey,
  } = modelContext;
  const providerGroups = useMemo(() => {
    const groups = new Map<string, FreeModelProviderGroup>();
    models.forEach((model) => {
      const id = `${model.providerId}:${model.baseUrl}`;
      const group = groups.get(id);
      if (group) group.models.push(model);
      else {
        groups.set(id, {
          id,
          name: model.provider,
          baseUrl: model.baseUrl,
          docsUrl: model.docsUrl,
          models: [model],
        });
      }
    });
    return [...groups.values()].sort((left, right) => {
      return providerRank(left.models[0]) - providerRank(right.models[0]);
    });
  }, [models]);

  const openProvider = useCallback(
    (group: FreeModelProviderGroup) => {
      const modelIds = group.models.map((model) => model.modelId);
      setNewModelForm({
        name: group.name,
        baseUrl: group.baseUrl,
        anthropicUrl: group.models[0]?.anthropicUrl ?? '',
        apiKey: '',
        modelId: modelIds[0] ?? '',
        modelIdOptions: modelIds,
        freeModelIds: group.models
          .filter((model) => model.freeType === 'perpetual' || isKeylessFreeModel(model))
          .map((model) => model.modelId),
      });
      setEditingModelId(null);
      setModelModalDestination('freeRouter');
      setShowAddModelModal(true);
    },
    [setEditingModelId, setModelModalDestination, setNewModelForm, setShowAddModelModal]
  );

  const openCustomModel = () => {
    setNewModelForm({ name: '', baseUrl: '', anthropicUrl: '', apiKey: '', modelId: '' });
    setEditingModelId(null);
    setKeyDestroyed(false);
    setShowApiKey(false);
    setModelModalDestination('freeRouter');
    setShowAddModelModal(true);
  };

  const addSavedModel = async (model: (typeof userModels)[number]) => {
    if (addingId || selectedIds.has(model.internalId)) return;
    setAddingId(model.internalId);
    try {
      await addSelectedModel({
        internalId: model.internalId,
        name: model.name,
        baseUrl: model.baseUrl,
        anthropicUrl: model.anthropicUrl,
        apiKey: model.apiKey,
        modelId: model.modelId ?? '',
        accountSource: model.accountSource,
        scope: model.scope,
      });
    } catch (error) {
      console.error('Add saved model to smart router failed:', error);
      showToast('error', t('freeModels.saved.addFailed'));
    } finally {
      setAddingId(null);
    }
  };

  return (
    <div className="free-model-router h-full min-h-0 flex flex-col">
      <div className="h-10 shrink-0 px-2 mb-2 flex items-center justify-between gap-2">
        <ViewModeTabGroup
          role="tablist"
          className={`view-mode-tabs min-w-0 flex border border-cyber-border rounded-button overflow-hidden ${compactTabs ? 'flex-1' : ''}`}
        >
          {(['saved', 'account', 'free'] as const).map((tab, index, tabs) => (
            <button
              key={tab}
              type="button"
              role="tab"
              id={`router-tab-${tab}`}
              aria-selected={activeTab === tab}
              aria-controls={`router-panel-${tab}`}
              tabIndex={activeTab === tab ? 0 : -1}
              onClick={() => setActiveTab(tab)}
              onKeyDown={(event) => {
                if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
                event.preventDefault();
                const next =
                  event.key === 'Home'
                    ? 0
                    : event.key === 'End'
                      ? tabs.length - 1
                      : (index + (event.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length;
                setActiveTab(tabs[next]);
                const tabButtons =
                  event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>(
                    '[role="tab"]'
                  );
                tabButtons?.[next]?.focus();
              }}
              className={`view-mode-tab min-w-0 whitespace-nowrap py-1.5 transition-[color] ${
                compactTabs ? 'flex-auto px-[2px] text-[12px] font-sans' : 'px-3 text-sm font-mono'
              } ${
                activeTab === tab
                  ? 'bg-cyber-elevated text-cyber-text'
                  : 'text-cyber-text-secondary hover:text-cyber-text'
              }`}
            >
              {t(
                tab === 'saved'
                  ? 'freeModels.tab.saved'
                  : tab === 'account'
                    ? 'freeModels.tab.account'
                    : 'freeModels.tab.free'
              )}
            </button>
          ))}
        </ViewModeTabGroup>
        <button
          type="button"
          disabled={activeTab === 'account' && !accountRiskAcknowledged}
          onClick={() => {
            if (activeTab === 'account') {
              if (accountRiskAcknowledged) openAccountModelForm('codex', modelContext);
            } else openCustomModel();
          }}
          aria-label={t('freeModels.customAdd')}
          className="w-9 h-9 shrink-0 flex items-center justify-center border border-cyber-border rounded-button text-cyber-text-secondary enabled:hover:text-cyber-text enabled:hover:bg-cyber-elevated disabled:opacity-40 transition-colors"
        >
          <Plus size={16} />
        </button>
      </div>
      <div
        role="tabpanel"
        id={`router-panel-${activeTab}`}
        aria-labelledby={`router-tab-${activeTab}`}
        className="flex-1 min-h-0 flex flex-col"
      >
        {activeTab === 'saved' ? (
          <div className="flex-1 px-2 pt-0.5 pb-2 overflow-y-auto">
            {isLoadingModels ? (
              <div className="h-full flex items-center justify-center" aria-busy="true">
                <RefreshCw size={18} className="animate-spin text-cyber-text-muted" />
              </div>
            ) : userModels.length === 0 ? (
              <div className="h-full flex flex-col items-center justify-center gap-3 text-xs">
                <p className="text-cyber-text-muted">{t('freeModels.saved.empty')}</p>
                <button
                  type="button"
                  onClick={() => setActivePage('models')}
                  className="text-cyber-accent hover:underline"
                >
                  {t('freeModels.saved.manage')}
                </button>
              </div>
            ) : (
              <div className="space-y-2">
                {userModels.map((model) => {
                  const selected = selectedIds.has(model.internalId);
                  const adding = addingId === model.internalId;
                  return (
                    <ModelListCard
                      compact
                      selected={selected}
                      key={model.internalId}
                      model={model}
                      onSelect={() => void addSavedModel(model)}
                      selectionDisabled={selected || Boolean(addingId)}
                      selectionLabel={`${t(selected ? 'freeModels.saved.added' : 'freeModels.addToRouter')}: ${model.name} — ${model.modelId ?? ''}`}
                      selection={
                        adding ? (
                          <RefreshCw
                            size={22}
                            className="shrink-0 animate-spin text-cyber-text-muted"
                          />
                        ) : selected ? (
                          <Check
                            size={22}
                            strokeWidth={2.5}
                            className="shrink-0 text-cyber-accent"
                          />
                        ) : (
                          <Plus
                            size={22}
                            strokeWidth={2.5}
                            className="shrink-0 text-cyber-text-muted"
                          />
                        )
                      }
                      usage={modelUsageData[model.internalId]}
                      refreshing={isRefreshingUsage || refreshingUsageIds.has(model.internalId)}
                      onRefreshUsage={(modelId) =>
                        volcAkSkMissingIds.has(modelId)
                          ? openAkskModal(modelId)
                          : refreshSingleUsage(modelId)
                      }
                      onEditModel={handleCardEdit}
                      onDeleteModel={async (modelId) => {
                        const ok = await confirm({
                          title: t('model.deleteTitle'),
                          message: t('model.deleteConfirm'),
                          confirmText: t('btn.delete'),
                          cancelText: t('btn.cancel'),
                          type: 'danger',
                        });
                        if (!ok) return;
                        try {
                          await handleCardDelete(modelId);
                          if (selectedIds.has(modelId)) await removeSelectedModel(modelId);
                        } catch (error) {
                          console.error('Delete saved router model failed:', error);
                          showToast('error', t('error.requestFailed'));
                        }
                      }}
                      t={t}
                    />
                  );
                })}
              </div>
            )}
          </div>
        ) : activeTab === 'account' ? (
          <AccountProxyPanel />
        ) : (
          <div className="flex-1 min-h-0 flex flex-col overflow-y-auto">
            <div className="h-10 shrink-0 px-2 flex items-center">
              <button
                type="button"
                onClick={() => void refresh()}
                disabled={refreshing}
                className={`flex-1 h-9 px-3 text-[14px] font-semibold border rounded-button transition-colors flex items-center justify-center gap-2 ${
                  !refreshing
                    ? 'border-cyber-border text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated'
                    : 'border-cyber-border text-cyber-text-muted cursor-not-allowed'
                }`}
              >
                <RefreshCw size={14} className={refreshing ? 'animate-spin' : ''} />
                {t('freeModels.fetch')}
              </button>
            </div>
            {refreshing && (
              <div className="px-2 pb-2">
                <div className="h-1 rounded-full overflow-hidden bg-cyber-border/30">
                  <div
                    className="free-model-scan-progress h-full rounded-full bg-cyber-accent"
                    style={{ width: `${Math.max(scanProgress, 0.04) * 100}%` }}
                  />
                </div>
                <div className="mt-2 text-[11px] text-cyber-text-secondary truncate">
                  {scanProvider ? (
                    <>
                      {t('freeModels.scanning')}
                      {scanProvider}
                    </>
                  ) : (
                    <>
                      {t('freeModels.scanConnecting')}
                      <span className="free-model-scan-dots" aria-hidden="true">
                        <span>.</span>
                        <span>.</span>
                        <span>.</span>
                      </span>
                    </>
                  )}
                </div>
              </div>
            )}
            <div className="flex-1 p-2">
              {providerGroups.length === 0 ? (
                <div className="h-full flex items-center justify-center">
                  {!refreshing && (
                    <div className="text-xs text-cyber-text-muted text-center">
                      {t('freeModels.fetchHint')}
                    </div>
                  )}
                </div>
              ) : (
                <div className="space-y-2">
                  {providerGroups.map((group) => (
                    <FreeModelProviderRow
                      key={group.id}
                      group={group}
                      selected={group.models.some((model) =>
                        customModels.some(
                          (routeModel) =>
                            selectedIds.has(routeModel.internalId) &&
                            routeModel.baseUrl === model.baseUrl &&
                            routeModel.modelId === model.modelId
                        )
                      )}
                      onAdd={() => openProvider(group)}
                    />
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
