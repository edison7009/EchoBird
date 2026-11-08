import { isSmartRouterId, SMART_ROUTER_SWITCH_TOOLS } from '../../utils/smartRouter';
import { CursorAccountSection } from './CursorAccountSection';
import { AntigravityAccountSection } from './AntigravityAccountSection';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from './AccountSectionPrimitives';
import { DeepSeekAccountSection } from './DeepSeekAccountSection';
import { GrokAccountSection } from './GrokAccountSection';
import { ClientAccountSection } from './ClientAccountSection';
import { ClineAccountSection } from './ClineAccountSection';
import { ManusAccountSection } from './ManusAccountSection';
import { ModelSwitchDivider } from './ModelSwitchDivider';
import { QuotaCountdown } from './QuotaCountdown';
import { QuotaPercent } from './QuotaPercent';
import { quotaTextClass } from '../../utils/quotaColors';
import { codexPlanLabel } from './codexPlanLabel';
import { WorkBuddyAccountSection } from './WorkBuddyAccountSection';
import { TraeAccountSection } from './TraeAccountSection';
import { KiroAccountSection } from './KiroAccountSection';
import { ZCodeAccountSection } from './ZCodeAccountSection';
import { PluginToolAccountSection } from './PluginToolAccountSection';
import { KimiAccountSection } from './KimiAccountSection';
import { CodeArtsAccountSection } from './CodeArtsAccountSection';
import { MinimaxAccountSection } from './MinimaxAccountSection';
import { QoderAccountSection } from './QoderAccountSection';
import { DuMateAccountSection } from './DuMateAccountSection';
import { DoubaoAccountSection } from './DoubaoAccountSection';
import { QwenWorkAccountSection } from './QwenWorkAccountSection';
import { ClaudeCodeAccountSection } from './ClaudeCodeAccountSection';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { CapabilityToggles, type CapabilityToggle } from './CapabilityToggles';
import { ModelListCard } from '../../components/ModelListCard';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { ViewModeTabGroup } from '../../components/ViewModeTabGroup';
import { PageActionButton } from '../../components/PageActionButton';
import {
  DndContext,
  PointerSensor,
  KeyboardSensor,
  closestCenter,
  DragOverlay,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  useSortable,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
  arrayMove,
} from '@dnd-kit/sortable';
import { useIsomorphicLayoutEffect } from '@dnd-kit/utilities';
import { Box as BoxIcon, ExternalLink, Gift, RefreshCw, Settings, Zap } from 'lucide-react';
import { getModelIcon } from '../../components';
import { useConfirm } from '../../components/ConfirmDialog';
import { CliLaunchDialog } from './CliLaunchDialog';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { ModelConfig, LocalTool } from '../../api/types';
import type { TKey } from '../../i18n';
import { useAppManager } from './context';
import { DevinAccountSection } from './DevinAccountSection';
import { OpenDesignAccountSection } from './OpenDesignAccountSection';
import { useNavigationStore } from '../../stores/navigationStore';
import { useModelNexus } from '../ModelNexus/context';
import { useDesktopTools } from './useDesktopTools';
import {
  getOfficialEndpoint,
  officialModelSentinel,
  type OfficialEndpoint,
} from '../../data/officialEndpoints';

// ===== Title actions (refresh) — mounted in the shared page title bar,
// keeping App Desktop consistent with the other pages =====

export const AppManagerTitleActions: React.FC = () => {
  const { t } = useI18n();
  const { scanTools, isScanning, viewMode, setViewMode } = useAppManager();
  const appsTab = useNavigationStore((state) => state.appsTab);
  const setAppsTab = useNavigationStore((state) => state.setAppsTab);

  return (
    <div className="ml-auto flex-shrink-0 flex items-center gap-3">
      <div className="flex items-center gap-2">
        {(appsTab === 'desktop' || appsTab === 'install') && (
          <>
            <PageActionButton
              icon={Settings}
              onClick={() => {
                void api.openToolPathsConfig().catch(() => {});
              }}
              aria-label={t('btn.editPaths')}
            >
              {t('btn.editPaths')}
            </PageActionButton>
            <PageActionButton
              icon={RefreshCw}
              spinning={isScanning}
              onClick={scanTools}
              disabled={isScanning}
            >
              {t('btn.refresh')}
            </PageActionButton>
          </>
        )}
        <ViewModeTabs
          dividers
          value={appsTab === 'desktop' || appsTab === 'install' ? viewMode : null}
          onChange={setViewMode}
          options={[
            { value: 'desktop', label: t('aiDesktop.desktopView') },
            { value: 'install', label: t('aiDesktop.installView') },
          ]}
        />
      </div>
      <div className="flex items-center gap-2">
        {(['pets', 'plugins', 'usageIsland'] as const).map((tab) => (
          <ViewModeTabGroup
            key={tab}
            className="view-mode-tabs h-9 overflow-hidden rounded-button border border-cyber-border"
          >
            <PageActionButton
              aria-pressed={appsTab === tab}
              className="view-mode-tab"
              style={{ borderWidth: 0, height: '100%' }}
              onClick={() => setAppsTab(tab)}
            >
              {t(`developer.${tab}`)}
            </PageActionButton>
          </ViewModeTabGroup>
        ))}
      </div>
    </div>
  );
};

// ===== Main Content (App Desktop grid) =====

// Category order for the "未安装" (not installed) grouping. The installed
// section renders flat (no category headers per spec); only the uninstalled
// section groups by category with i18n titles.
const CATEGORY_ORDER = [
  'Desktop',
  'Cloud Agent',
  'IDE',
  'CLI Code',
  'Science',
  'Creative',
  'AutoTrading',
  'Game',
  'Utility',
];

// Within Desktop, keep the fixed display order (Coffee CLI last).
const DESKTOP_ORDER: Record<string, number> = {
  claudedesktop: 0,
  chatgptdesktop: 1,
  geminidesktop: 2,
  kimidesktop: 3,
  coffeecli: 99,
};

const categoryRank = (cat?: string): number => {
  const idx = CATEGORY_ORDER.indexOf(cat || '');
  return idx === -1 ? 99 : idx;
};

// Within-category tiebreaker: Desktop keeps its fixed display order (Coffee
// CLI last).
const withinCategoryRank = (tool: LocalTool): number => {
  if (tool.category === 'Desktop') return DESKTOP_ORDER[tool.id] ?? 50;
  return 0;
};

// Stable order across the desktop: category rank, then the within-category
// tiebreaker, then name.
const compareTools = (a: LocalTool, b: LocalTool): number => {
  const catDiff = categoryRank(a.category) - categoryRank(b.category);
  if (catDiff !== 0) return catDiff;
  const rankDiff = withinCategoryRank(a) - withinCategoryRank(b);
  if (rankDiff !== 0) return rankDiff;
  return a.name.localeCompare(b.name);
};

const catLabelKey = (cat: string): TKey => {
  const map: Record<string, TKey> = {
    IDE: 'toolCat.ide',
    'CLI Code': 'toolCat.cli',
    AutoTrading: 'toolCat.autoTrading',
    Game: 'toolCat.game',
    Desktop: 'toolCat.desktop',
    'Cloud Agent': 'toolCat.cloudAgent',
    Utility: 'toolCat.utility',
    Science: 'toolCat.science',
    Creative: 'toolCat.creative',
  };
  return map[cat] || (cat as TKey);
};

// Localized display name — resolves per-locale `names`, but
// prefers `displayName` when present (the pre-localized label some tools
// carry), then falls back to the plain name.
const toolDisplayName = (tool: LocalTool, locale: string): string => {
  if (tool.displayName) return tool.displayName;
  if (tool.names && locale !== 'en') {
    return (
      tool.names[locale] ||
      tool.names[locale.split('-')[0]] ||
      Object.entries(tool.names).find(([k]) => k.startsWith(locale.split('-')[0]))?.[1] ||
      tool.name
    );
  }
  return tool.name;
};

interface DesktopIconProps {
  tool: LocalTool;
  selected: boolean;
  onClick: () => void;
  /** dnd-kit drag attributes/listeners (sortable tiles only). Applied to the
      button itself so the tile stays a single focusable control instead of
      nesting a button inside a role="button" wrapper. */
  dragProps?: React.HTMLAttributes<HTMLElement>;
}

// A desktop-style launcher tile: icon on top, name beneath. Clicking selects;
// the bottom bar holds the launch / install action. All icons render
// uniformly — which section an app sits in (已安装 / 未安装) tells the state.
const DesktopIcon: React.FC<DesktopIconProps> = ({ tool, selected, onClick, dragProps }) => {
  const { locale } = useI18n();
  const iconSrc = tool.icon;
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const displayName = toolDisplayName(tool, locale);

  return (
    <button
      {...dragProps}
      onClick={onClick}
      aria-label={displayName}
      data-desktop-tool-id={tool.id}
      className={`flex w-full flex-col items-center gap-1.5 rounded-xl border-2 px-1.5 py-3 outline-none transition-colors select-none focus-visible:ring-2 focus-visible:ring-cyber-accent ${
        selected
          ? 'border-cyber-accent/70 bg-cyber-accent/10'
          : 'border-transparent hover:bg-cyber-elevated/60'
      }`}
    >
      {/* The icon alone is the graphic — no tile background behind it; the
          icon itself renders at the tile size. */}
      <span className="relative flex h-14 w-14 shrink-0 items-center justify-center">
        {iconSrc && failedSrc !== iconSrc ? (
          <img
            src={iconSrc}
            alt=""
            draggable={false}
            decoding="sync"
            onError={() => setFailedSrc(iconSrc)}
            className="w-14 h-14 object-contain"
          />
        ) : (
          <BoxIcon size={44} className="text-cyber-text-secondary" />
        )}
      </span>
      {/* Reserve exactly two lines for every name. Single-line labels center
          vertically in the same area occupied by wrapped two-line labels. */}
      <span className="flex h-8 w-full items-center justify-center text-center text-xs !leading-tight text-cyber-text">
        <span className="line-clamp-2 break-words break-keep">{displayName}</span>
      </span>
    </button>
  );
};

// Sortable wrapper shared by My and Store icons.
// The wrapper is the grid item; the tile inside fills it (w-full) so the
// drag handles and the click-to-select behavior stay aligned.
const SortableDesktopIcon: React.FC<DesktopIconProps> = ({ tool, selected, onClick }) => {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: tool.id,
  });
  const style: React.CSSProperties = {
    // Keep fractional grid coordinates so committing the order does not nudge icons.
    transform: transform ? `translate3d(${transform.x}px, ${transform.y}px, 0)` : undefined,
    transition,
    // While dragging, fade the item at its sort position into a translucent
    // placeholder — that's the insertion indicator — while the DragOverlay
    // ghost carries the visuals following the cursor.
    opacity: isDragging ? 0.3 : undefined,
    zIndex: isDragging ? 10 : undefined,
  };
  return (
    <div ref={setNodeRef} style={style} className="flex">
      <DesktopIcon
        tool={tool}
        selected={selected}
        onClick={onClick}
        dragProps={{ ...attributes, ...listeners }}
      />
    </div>
  );
};

export const AppManagerMain: React.FC = () => {
  const { t } = useI18n();
  const { detectedTools, isScanning, selectedTool, setSelectedTool, aiInstallableIds, viewMode } =
    useAppManager();
  // Active category tab for the "未安装" section. 'ALL' shows every
  // uninstalled app; the other tabs filter by category.
  const [activeUninstalledCat, setActiveUninstalledCat] = useState('ALL');
  const categoryTabsRef = useRef<HTMLDivElement>(null);
  const categoryUnderlineRef = useRef<HTMLSpanElement>(null);

  // Persist My and Store orders separately; retain the existing My key.
  const orderKeys = {
    desktop: 'echobird_appmgr_tool_order',
    install: 'echobird_appmgr_store_order',
  };
  const [toolOrders, setToolOrders] = useState(() => {
    const readOrder = (key: string): string[] => {
      try {
        const v = localStorage.getItem(key);
        return v ? (JSON.parse(v) as string[]) : [];
      } catch {
        return [];
      }
    };
    return { desktop: readOrder(orderKeys.desktop), install: readOrder(orderKeys.install) };
  });
  const saveToolOrder = (ids: string[]) => {
    setToolOrders((prev) => ({ ...prev, [viewMode]: ids }));
    try {
      localStorage.setItem(orderKeys[viewMode], JSON.stringify(ids));
    } catch {
      /* private mode */
    }
  };

  const installed = useMemo(
    () =>
      detectedTools
        .filter((tool) => tool.installed && !['reversi', 'translator'].includes(tool.id))
        .sort(compareTools),
    [detectedTools]
  );
  const uninstalled = useMemo(
    () => detectedTools.filter((tool) => !tool.installed),
    [detectedTools]
  );

  // Apply the saved order on top of the default sort: known ids first in
  // saved order, then any freshly-detected tools in default order.
  const installedOrdered = useMemo(() => {
    const orderIndex = new Map(toolOrders.desktop.map((id, i) => [id, i]));
    const known = installed.filter((t) => orderIndex.has(t.id));
    const unknown = installed.filter((t) => !orderIndex.has(t.id));
    known.sort((a, b) => orderIndex.get(a.id)! - orderIndex.get(b.id)!);
    return [...known, ...unknown];
  }, [installed, toolOrders.desktop]);

  // Shared drag-reorder — pointer with a 5px activation so
  // plain clicks still select; keyboard for a11y. On drop, reorder in place
  // and persist the full view order (best-effort; a failed write just
  // reverts on next reload).
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );
  const [activeDragId, setActiveDragId] = useState<string | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const dropPositions = useRef<Map<string, DOMRect> | null>(null);
  const dropAnimations = useRef<Animation[]>([]);

  const dragScope = `${viewMode}:${activeUninstalledCat}`;
  const [previousDragScope, setPreviousDragScope] = useState(dragScope);
  if (previousDragScope !== dragScope) {
    setPreviousDragScope(dragScope);
    setActiveDragId(null);
  }

  // Category tabs present among the uninstalled apps: the canonical order
  // first, then any unknown categories alphabetically.
  const uninstalledCats = useMemo(() => {
    const cats = Array.from(new Set(uninstalled.map((t) => t.category).filter(Boolean)));
    return [
      ...CATEGORY_ORDER.filter((cat) => cats.includes(cat)),
      ...cats.filter((cat) => !CATEGORY_ORDER.includes(cat)).sort(),
    ];
  }, [uninstalled]);

  // Installing the last app in a category removes its tab; return to All.
  if (activeUninstalledCat !== 'ALL' && !uninstalledCats.includes(activeUninstalledCat)) {
    setActiveUninstalledCat('ALL');
  }

  useEffect(() => {
    const tabs = categoryTabsRef.current;
    const underline = categoryUnderlineRef.current;
    if (!tabs || !underline) return;

    let frame: number | undefined;
    const updateUnderline = () => {
      const active = tabs.querySelector<HTMLButtonElement>('[aria-pressed="true"]');
      if (!active?.offsetWidth) return;
      // Place the line immediately on entry; animate subsequent selections.
      if (!underline.style.width) {
        underline.style.transition = 'none';
        frame = requestAnimationFrame(() => underline.style.removeProperty('transition'));
      }
      underline.style.width = `${active.offsetWidth}px`;
      underline.style.transform = `translate(${active.offsetLeft}px, ${active.offsetTop + active.offsetHeight - 2}px)`;
      underline.style.visibility = 'visible';
    };
    updateUnderline();
    const observer = new ResizeObserver(updateUnderline);
    observer.observe(tabs);
    tabs.querySelectorAll('button').forEach((button) => observer.observe(button));
    return () => {
      observer.disconnect();
      if (frame !== undefined) {
        cancelAnimationFrame(frame);
        underline.style.removeProperty('transition');
      }
    };
  }, [activeUninstalledCat, uninstalledCats, viewMode, t]);

  // Saved Store order first; otherwise keep the existing default sort.
  const uninstalledOrdered = useMemo(() => {
    const orderIndex = new Map(toolOrders.install.map((id, i) => [id, i]));
    return [...uninstalled].sort((a, b) => {
      const aOrder = orderIndex.get(a.id) ?? Infinity;
      const bOrder = orderIndex.get(b.id) ?? Infinity;
      if (aOrder !== bOrder) return aOrder - bOrder;
      const aAi = aiInstallableIds.includes(a.id) ? 0 : 1;
      const bAi = aiInstallableIds.includes(b.id) ? 0 : 1;
      if (aAi !== bAi) return aAi - bAi;
      const rankDiff = withinCategoryRank(a) - withinCategoryRank(b);
      if (rankDiff !== 0) return rankDiff;
      return a.name.localeCompare(b.name);
    });
  }, [uninstalled, aiInstallableIds, toolOrders.install]);

  const visibleUninstalled =
    activeUninstalledCat === 'ALL'
      ? uninstalledOrdered
      : uninstalledOrdered.filter((tool) => tool.category === activeUninstalledCat);
  const orderedTools = viewMode === 'desktop' ? installedOrdered : uninstalledOrdered;
  const candidateTools = viewMode === 'desktop' ? installedOrdered : visibleUninstalled;
  const preparedDesktop = useDesktopTools(candidateTools, dragScope);
  const visibleTools = preparedDesktop.tools;
  const initialLoading =
    (isScanning && detectedTools.length === 0) ||
    (preparedDesktop.pending && visibleTools.length === 0);
  const loadingKey = preparedDesktop.pending ? preparedDesktop.key : 'scan';
  const [skeletonKey, setSkeletonKey] = useState('');
  useEffect(() => {
    if (!initialLoading) return;
    const timer = setTimeout(() => setSkeletonKey(loadingKey), 120);
    return () => clearTimeout(timer);
  }, [initialLoading, loadingKey]);
  const activeDragTool = visibleTools.find((tool) => tool.id === activeDragId);

  useEffect(
    () => () => {
      dropPositions.current = null;
      dropAnimations.current.forEach((animation) => animation.cancel());
      dropAnimations.current = [];
    },
    [dragScope]
  );

  useIsomorphicLayoutEffect(() => {
    const positions = dropPositions.current;
    dropPositions.current = null;
    if (!positions || !gridRef.current) return;
    dropAnimations.current.forEach((animation) => animation.cancel());
    dropAnimations.current = [];
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    Array.from(gridRef.current.children).forEach((element, index) => {
      const before = positions.get(visibleTools[index].id);
      if (!before) return;
      const after = element.getBoundingClientRect();
      const x = before.left - after.left;
      const y = before.top - after.top;
      if (Math.abs(x) < 0.01 && Math.abs(y) < 0.01) return;
      // Finish an in-flight insertion from its visible position, not its old grid cell.
      dropAnimations.current.push(
        element.animate(
          [{ transform: `translate3d(${x}px, ${y}px, 0)` }, { transform: 'translate3d(0, 0, 0)' }],
          { duration: 200, easing: 'ease' }
        )
      );
    });
  }, [visibleTools]);

  const handleDragStart = (event: DragStartEvent) => {
    if (preparedDesktop.pending) return;
    dropAnimations.current.forEach((animation) => animation.cancel());
    dropAnimations.current = [];
    setActiveDragId(String(event.active.id));
  };
  const handleDragEnd = (event: DragEndEvent) => {
    setActiveDragId(null);
    if (preparedDesktop.pending) return;
    const activeId = String(event.active.id);
    const overId = String(event.over?.id ?? '');
    if (!overId || activeId === overId) return;
    const oldIndex = visibleTools.findIndex((tool) => tool.id === activeId);
    const newIndex = visibleTools.findIndex((tool) => tool.id === overId);
    if (oldIndex < 0 || newIndex < 0) return;
    dropPositions.current = new Map(
      Array.from(gridRef.current?.children ?? []).flatMap((element, index) =>
        index === oldIndex ? [] : [[visibleTools[index].id, element.getBoundingClientRect()]]
      )
    );
    const reordered = arrayMove(visibleTools, oldIndex, newIndex).map((tool) => tool.id);
    const visibleIds = new Set(reordered);
    let index = 0;
    // A category drag replaces only its visible slots in the full Store order.
    saveToolOrder(
      orderedTools.map((tool) => (visibleIds.has(tool.id) ? reordered[index++] : tool.id))
    );
  };
  const handleDragCancel = () => setActiveDragId(null);

  const gridClass = 'grid grid-cols-[repeat(auto-fill,minmax(7rem,1fr))] gap-x-2 gap-y-4';
  const sortableGrid = (
    <DndContext
      key={dragScope}
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      <SortableContext items={visibleTools.map((tool) => tool.id)} strategy={rectSortingStrategy}>
        <div ref={gridRef} className={gridClass}>
          {visibleTools.map((tool) => (
            <SortableDesktopIcon
              key={tool.id}
              tool={tool}
              selected={selectedTool === tool.id}
              onClick={() => setSelectedTool(tool.id)}
            />
          ))}
        </div>
      </SortableContext>
      <DragOverlay>
        {activeDragTool && (
          <div className="pointer-events-none opacity-70 scale-105 drop-shadow-lg">
            <DesktopIcon tool={activeDragTool} selected={false} onClick={() => {}} />
          </div>
        )}
      </DragOverlay>
    </DndContext>
  );

  return (
    <div className="flex-1 flex flex-col overflow-hidden" aria-busy={initialLoading || undefined}>
      {initialLoading ? (
        <div
          className={gridClass}
          style={{ visibility: skeletonKey === loadingKey ? 'visible' : 'hidden' }}
        >
          {Array.from({ length: candidateTools.length || 8 }).map((_, i) => (
            <div
              key={i}
              className="flex flex-col items-center gap-1.5 border-2 border-transparent px-1.5 py-3 rounded-xl animate-pulse motion-reduce:animate-none"
            >
              <span className="w-14 h-14 rounded-xl bg-cyber-border/30" />
              <span className="flex h-8 items-center">
                <span className="w-12 h-3 bg-cyber-border/30 rounded" />
              </span>
            </div>
          ))}
        </div>
      ) : (
        <div key={viewMode} className="flex min-h-0 flex-1 flex-col">
          {/* Install view — category tabs filter the uninstalled apps. */}
          {viewMode === 'install' && uninstalled.length > 0 && (
            <div ref={categoryTabsRef} className="relative mb-5 flex shrink-0 flex-wrap gap-1">
              <button
                onClick={() => setActiveUninstalledCat('ALL')}
                aria-pressed={activeUninstalledCat === 'ALL'}
                className={`border-b-2 border-transparent px-3 py-1.5 text-[14px] font-bold transition-colors outline-none ${
                  activeUninstalledCat === 'ALL'
                    ? 'text-cyber-text'
                    : 'text-cyber-text-secondary hover:text-cyber-text'
                }`}
              >
                {t('toolCat.all')}
              </button>
              {uninstalledCats.map((cat) => (
                <button
                  key={cat}
                  onClick={() => setActiveUninstalledCat(cat)}
                  aria-pressed={activeUninstalledCat === cat}
                  className={`border-b-2 border-transparent px-3 py-1.5 text-[14px] font-bold transition-colors outline-none ${
                    activeUninstalledCat === cat
                      ? 'text-cyber-text'
                      : 'text-cyber-text-secondary hover:text-cyber-text'
                  }`}
                >
                  {t(catLabelKey(cat))}
                </button>
              ))}
              <span
                ref={categoryUnderlineRef}
                aria-hidden="true"
                className="pointer-events-none absolute left-0 top-0 h-0.5 bg-cyber-border transition-[transform,width] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:transition-none"
                style={{ visibility: 'hidden' }}
              />
            </div>
          )}
          <div className="min-h-0 flex-1 overflow-y-auto">
            {/* Installed — flat draggable grid, no section header (per spec) */}
            {viewMode === 'desktop' && installedOrdered.length > 0 && <div>{sortableGrid}</div>}
            {viewMode === 'install' && uninstalled.length > 0 && <section>{sortableGrid}</section>}
            {(viewMode === 'desktop' ? installed.length === 0 : uninstalled.length === 0) && (
              <p className="py-12 text-center text-sm text-cyber-text-secondary">
                {t(viewMode === 'desktop' ? 'aiDesktop.emptyDesktop' : 'aiDesktop.emptyInstall')}
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
};

// ===== Model List Section =====

interface ModelListSectionProps {
  smartRouterEnabled?: boolean;
  selectedToolData: LocalTool;
  userModels: ModelConfig[];
  toolModelConfig: Record<string, string | null>;
  selectedTool: string | null;
  handleSelectModel: (toolId: string, modelId: string) => void;
  modelUsageData?: Record<string, api.ModelUsageData>;
  refreshingUsageIds?: Set<string>;
  isRefreshingUsage?: boolean;
  onRefreshUsage?: (modelId: string) => void;
  onEditModel?: (model: ModelConfig) => void;
  onDeleteModel?: (modelId: string) => void;
  t: (key: TKey) => string;
}

function isModelCompatibleWithTool(
  model: ModelConfig,
  toolProtocols: string[],
  selectedTool: string | null
): boolean {
  const requiresResponses = selectedTool === 'codex' || selectedTool === 'chatgptdesktop';
  if (requiresResponses && model.internalId === 'local-server') return false;

  const hasOpenAI = toolProtocols.includes('openai') && !!model.baseUrl;
  const hasAnthropic = toolProtocols.includes('anthropic') && !!model.anthropicUrl;
  return hasOpenAI || hasAnthropic;
}

export const ModelListSection: React.FC<ModelListSectionProps> = ({
  smartRouterEnabled = true,
  selectedToolData,
  userModels,
  toolModelConfig,
  selectedTool,
  handleSelectModel,
  modelUsageData,
  refreshingUsageIds,
  isRefreshingUsage,
  onRefreshUsage,
  onEditModel,
  onDeleteModel,
  t,
}) => {
  const toolProtocols = useMemo(
    () => selectedToolData.apiProtocol || ['openai', 'anthropic'],
    [selectedToolData.apiProtocol]
  );

  const { smartRouterModels, localModels, cloudModels } = useMemo(() => {
    const compatible = userModels.filter(
      (model) =>
        isModelCompatibleWithTool(model, toolProtocols, selectedTool) &&
        (!isSmartRouterId(model.internalId) || smartRouterEnabled)
    );
    return {
      smartRouterModels: compatible.filter((m) => isSmartRouterId(m.internalId)),
      localModels: compatible.filter((m) => m.internalId === 'local-server'),
      cloudModels: compatible.filter(
        (m) => m.internalId !== 'local-server' && !isSmartRouterId(m.internalId)
      ),
    };
  }, [userModels, toolProtocols, selectedTool, smartRouterEnabled]);

  const renderModelCard = (model: (typeof userModels)[0], badge?: 'smart' | 'local') => {
    const isSelected = selectedTool ? toolModelConfig[selectedTool] === model.internalId : false;
    const modelHasBoth = !!(model.baseUrl && model.anthropicUrl);
    // Use the same default as applyModelConfig: a model with one URL uses that
    // URL's protocol; a model with both URLs follows the tool's first protocol.
    const currentProtocol = modelHasBoth
      ? toolProtocols[0] === 'anthropic'
        ? 'anthropic'
        : 'openai'
      : model.anthropicUrl
        ? 'anthropic'
        : 'openai';

    const displayUrl =
      currentProtocol === 'anthropic'
        ? model.anthropicUrl || model.baseUrl
        : model.baseUrl || model.anthropicUrl;
    const apiPath = (() => {
      try {
        const url = new URL(displayUrl || '');
        const path = url.pathname === '/' ? '' : url.pathname;
        return url.host + path;
      } catch {
        return displayUrl || 'No URL Configured';
      }
    })();

    return (
      <ModelListCard
        compact
        key={model.internalId}
        model={model}
        selected={isSelected}
        subtitle={isSmartRouterId(model.internalId) ? apiPath : undefined}
        onSelect={() => selectedTool && handleSelectModel(selectedTool, model.internalId)}
        selection={null}
        badge={
          badge && (
            <span
              className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium leading-none ${
                badge === 'smart'
                  ? 'bg-cyber-accent/10 text-cyber-accent'
                  : 'bg-cyber-text/10 text-cyber-text-secondary'
              }`}
            >
              {t(badge === 'smart' ? 'agent.badge.smart' : 'agent.badge.local')}
            </span>
          )
        }
        secondaryBadge={
          badge === 'smart' &&
          selectedTool &&
          SMART_ROUTER_SWITCH_TOOLS.has(selectedTool) && (
            <span className="model-switch-badge shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium leading-none bg-emerald-500/10 text-emerald-700">
              {t('agent.badge.switch')}
            </span>
          )
        }
        usage={modelUsageData?.[model.internalId]}
        refreshing={isRefreshingUsage || refreshingUsageIds?.has(model.internalId)}
        onRefreshUsage={onRefreshUsage}
        onEditModel={onEditModel}
        onDeleteModel={onDeleteModel}
        t={t}
      />
    );
  };

  // Official-endpoint card — first item.
  const accountReplacesOfficial =
    selectedTool === 'codex' ||
    selectedTool === 'chatgptdesktop' ||
    selectedTool === 'claudecode' ||
    selectedTool === 'grok' ||
    selectedTool === 'cline' ||
    selectedTool === 'clinedesktop' ||
    selectedTool === 'kilo' ||
    selectedTool === 'kilodesktop' ||
    selectedTool === 'copilot' ||
    selectedTool === 'copilotdesktop' ||
    selectedTool === 'workbuddy' ||
    selectedTool === 'workbuddyai';
  const official =
    selectedTool && !accountReplacesOfficial ? getOfficialEndpoint(selectedTool) : undefined;
  const officialSentinel = selectedTool ? officialModelSentinel(selectedTool) : '';
  const isOfficialPending = !!(selectedTool && toolModelConfig[selectedTool] === officialSentinel);

  const renderOfficialCard = (ep: OfficialEndpoint) => {
    const apiPath = (() => {
      try {
        const url = new URL(
          ep.protocol === 'anthropic' ? ep.anthropicUrl || ep.baseUrl : ep.baseUrl
        );
        const path = url.pathname === '/' ? '' : url.pathname;
        return url.hostname + path;
      } catch {
        return ep.baseUrl;
      }
    })();

    // Use provider icon (Claude/OpenAI etc.) based on official endpoint name
    const iconSrc = getModelIcon(ep.name, ep.modelId);

    return (
      <div
        className={`relative overflow-hidden h-14 px-[11px] py-1 rounded-card cursor-default transition-colors flex items-center gap-3 border-2 ${
          isOfficialPending
            ? 'border-cyber-accent/70 bg-cyber-accent/10'
            : 'bg-cyber-surface border-transparent hover:bg-cyber-elevated'
        }`}
        onClick={() => selectedTool && handleSelectModel(selectedTool, officialSentinel)}
      >
        <div className="relative z-10 flex items-center gap-3 flex-shrink-0">
          {iconSrc ? (
            <img
              src={iconSrc}
              alt=""
              className="w-8 h-8"
              onError={(e) => {
                (e.target as HTMLImageElement).style.display = 'none';
              }}
            />
          ) : (
            <div className="w-8 h-8 rounded bg-cyber-text/15 flex items-center justify-center text-cyber-text">
              <BoxIcon size={24} />
            </div>
          )}
        </div>
        <div className="relative z-10 flex-1 min-w-0 grid grid-cols-[minmax(0,1fr)] auto-rows-[16px] items-center">
          <div className="flex items-center gap-2">
            <div className="text-sm font-bold truncate leading-none flex-1 min-w-0">{ep.name}</div>
            <span className="text-xs font-mono text-cyber-text-secondary/60 flex-shrink-0 pointer-events-none select-none">
              {t('agent.restore')}
            </span>
          </div>
          <div className="text-[10px] text-cyber-text-secondary truncate leading-tight opacity-70">
            {apiPath}
          </div>
        </div>
      </div>
    );
  };

  // Fully empty: no local models, no cloud models, no official endpoint.
  // Show only the centered placeholder — the "select model for X" heading
  // would be misleading when there's nothing to select anyway.
  const isEmpty =
    cloudModels.length === 0 &&
    !official &&
    localModels.length === 0 &&
    smartRouterModels.length === 0;
  if (isEmpty) {
    const [before, after] = t('agent.noModelsHint').split('{modelNexus}');
    return (
      <div className="flex h-full items-center justify-center text-center">
        <p className="text-base text-cyber-text-secondary font-mono leading-relaxed">
          {before}
          <span className="text-cyber-text font-bold">{t('nav.modelNexus')}</span>
          {after}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {smartRouterModels.map((model) => renderModelCard(model, 'smart'))}
      {localModels.map((model) => renderModelCard(model, 'local'))}
      {official && renderOfficialCard(official)}
      {cloudModels.map((model) => renderModelCard(model))}
    </div>
  );
};

export const CodexAccountSection: React.FC<{ showDivider?: boolean }> = ({
  showDivider = true,
}) => {
  const {
    codexAccounts,
    selectedCodexAccountId,
    setSelectedCodexAccountId,
    isLoadingCodexAccounts,
    isAddingCodexAccount,
    codexOAuthRemainingSeconds,
    refreshingCodexAccountIds,
    codexAuthorizationFailedIds,
    addCodexAccount,
    cancelCodexLogin,
    refreshCodexAccountQuota,
    deleteCodexAccount,
  } = useAppManager();

  return (
    <AccountSection>
      <AccountSectionButton
        iconSrc="/icons/tools/codex.svg"
        busy={isAddingCodexAccount}
        disabled={isLoadingCodexAccounts}
        remainingSeconds={codexOAuthRemainingSeconds}
        onClick={() => void addCodexAccount()}
        onCancel={cancelCodexLogin}
      />
      {codexAccounts.length > 0 && (
        <div className="space-y-2">
          {codexAccounts.map((account) => {
            const planLabel = codexPlanLabel(account.plan);
            const singleWindow =
              account.quotaWindows?.length === 1 ? account.quotaWindows[0] : null;
            const quotaPercent = singleWindow
              ? singleWindow.remainingPercent
              : account.quotaPercent;
            const quotaResetAt = singleWindow ? singleWindow.resetAt : account.quotaResetAt;
            return (
              <AccountSectionRow
                key={account.id}
                selected={selectedCodexAccountId === account.id}
                email={account.email}
                plan={planLabel}
                planPrefix={
                  account.subscriptionEndAt ? (
                    <QuotaCountdown resetAt={account.subscriptionEndAt} compact />
                  ) : undefined
                }
                refreshing={refreshingCodexAccountIds.has(account.id)}
                authorizationFailed={codexAuthorizationFailedIds.has(account.id)}
                onSelect={() => setSelectedCodexAccountId(account.id)}
                onRefresh={() => void refreshCodexAccountQuota(account)}
                onDelete={() => void deleteCodexAccount(account)}
                secondary={
                  !account.quotaWindows?.some((window) => window.remainingPercent != null) &&
                  quotaPercent == null ? undefined : account.quotaWindows &&
                    account.quotaWindows.length > 1 ? (
                    <span className="flex h-[16px] min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[11px] font-semibold leading-[16px] text-cyber-text">
                      {account.quotaWindows.map((window, index) => (
                        <span
                          key={index}
                          className={`flex flex-shrink-0 items-center ${quotaTextClass(window.remainingPercent)}`}
                        >
                          <QuotaPercent percent={window.remainingPercent} />
                          <QuotaCountdown resetAt={window.resetAt} compact small parenthesized />
                        </span>
                      ))}
                    </span>
                  ) : (
                    <span
                      className={`flex h-[16px] items-center justify-between ${quotaTextClass(quotaPercent)}`}
                    >
                      <span className="h-1.5 min-w-[56px] max-w-[80px] flex-1 overflow-hidden rounded-full account-quota-track">
                        <span
                          className="quota-progress-fill block h-full rounded-full bg-cyber-bg"
                          style={{ width: `${quotaPercent ?? 0}%` }}
                        />
                      </span>
                      <span className="ml-[6px] w-[30px] flex-shrink-0 text-right text-[12px] font-semibold leading-[16px] text-cyber-text">
                        <QuotaPercent percent={quotaPercent} />
                      </span>
                      <QuotaCountdown resetAt={quotaResetAt} />
                    </span>
                  )
                }
              />
            );
          })}
        </div>
      )}
      {showDivider && <ModelSwitchDivider />}
    </AccountSection>
  );
};

// ===== Right Panel (config panel with tabs) =====

export const AppManagerPanel: React.FC = () => {
  const { t, locale } = useI18n();
  const { smartRouterEnabled } = useAppManager();
  const confirm = useConfirm();
  const {
    modelUsageData,
    refreshingUsageIds,
    isRefreshingUsage,
    refreshSingleUsage,
    handleCardEdit,
    handleCardDelete,
    volcAkSkMissingIds,
    openAkskModal,
  } = useModelNexus();
  const {
    selectedToolData,
    selectedTool,
    userModels,
    toolModelConfig,
    handleSelectModel,
    claudeDesktop1mMode,
    setClaudeDesktop1mMode,
    claude1mMode,
    setClaude1mMode,
    codexWebSearch,
    setCodexWebSearch,
    claudeDesktopWebSearch,
    claudeCodeWebSearch,
    setClaudeWebSearch,
    zcodeWebSearch,
    zcodeImageInput,
    setZCodeCapability,
    workBuddyImageInput,
    setWorkBuddyImageInput,
    workBuddy1mMode,
    setWorkBuddy1mMode,
    mimoImageInput,
    mimo1mMode,
    setMimoCapability,
    minimaxImageInput,
    minimax1mMode,
    setMinimaxCapability,
    qoderImageInput,
    qoder1mMode,
    setQoderCapability,
    dshImageInput,
    dsh1mMode,
    setDshCapability,
    isLaunching,
    pluginAccounts,
    refreshSelectedTool,
    isRefreshingSelectedTool,
    claimSelectedTool,
    warmUpSelectedTool,
    selectedToolAction,
  } = useAppManager();

  const isClaudeDesktopApp = selectedTool === 'claudedesktop';
  const isClaudeCodeApp = selectedTool === 'claudecode';
  const isZCodeApp = selectedTool === 'zcode';
  const isWorkBuddyApp = selectedTool === 'workbuddy' || selectedTool === 'workbuddyai';
  const isMimoTool = selectedTool === 'mimocode' || selectedTool === 'mimodesktop';
  const isMinimaxTool = selectedTool === 'minimaxcode' || selectedTool === 'minimaxdesktop';
  const isQoderTool = selectedTool === 'qoder' || selectedTool === 'qodercn';
  const isDshTool = selectedTool === 'dsh';
  const showImageToggle =
    isZCodeApp || isWorkBuddyApp || isMimoTool || isMinimaxTool || isQoderTool || isDshTool;
  const show1mToggle =
    isClaudeDesktopApp ||
    isClaudeCodeApp ||
    isMimoTool ||
    isMinimaxTool ||
    isQoderTool ||
    isDshTool ||
    isWorkBuddyApp;
  const pluginWarmUp =
    selectedTool === pluginAccounts?.provider?.application && !!pluginAccounts.warmUp;
  const pluginClaim =
    selectedTool === pluginAccounts?.provider?.application && !!pluginAccounts.claimDaily;
  const supportsWarmUp = isClaudeCodeApp || isClaudeDesktopApp || pluginWarmUp;
  const supportsClaim =
    pluginClaim ||
    selectedTool === 'workbuddy' ||
    selectedTool === 'traecn' ||
    isMinimaxTool ||
    isQoderTool ||
    selectedTool === 'dumate' ||
    selectedTool === 'doubaowork' ||
    selectedTool === 'dola' ||
    selectedTool === 'codearts';
  const showCodexAccounts = selectedTool === 'codex' || selectedTool === 'chatgptdesktop';
  const selectedToolProtocols = selectedToolData?.apiProtocol || ['openai', 'anthropic'];
  const hasVisibleModels = userModels.some(
    (model) =>
      isModelCompatibleWithTool(model, selectedToolProtocols, selectedTool) &&
      (!isSmartRouterId(model.internalId) || smartRouterEnabled)
  );

  const showWebSearchToggle =
    showCodexAccounts || isClaudeDesktopApp || isClaudeCodeApp || isZCodeApp;
  const webSearchValue = isClaudeDesktopApp
    ? claudeDesktopWebSearch
    : isClaudeCodeApp
      ? claudeCodeWebSearch
      : isZCodeApp
        ? zcodeWebSearch
        : codexWebSearch;
  const capabilityToggles: CapabilityToggle[] = [];
  if (showWebSearchToggle)
    capabilityToggles.push({
      label: t('agent.codexWebSearchLabel'),
      hint: t('agent.codexWebSearchHint'),
      checked: webSearchValue,
      onChange: showCodexAccounts
        ? setCodexWebSearch
        : isZCodeApp
          ? (value) => setZCodeCapability('webSearch', value)
          : (value) =>
              setClaudeWebSearch(isClaudeDesktopApp ? 'claudedesktop' : 'claudecode', value),
    });
  if (showImageToggle)
    capabilityToggles.push({
      label: 'Image',
      hint: t('agent.imageInputHint'),
      checked: isZCodeApp
        ? zcodeImageInput
        : isMimoTool
          ? mimoImageInput
          : isMinimaxTool
            ? minimaxImageInput
            : isQoderTool
              ? qoderImageInput
              : isDshTool
                ? dshImageInput
                : workBuddyImageInput,
      onChange: isZCodeApp
        ? (value) => setZCodeCapability('imageInput', value)
        : isMimoTool
          ? (value) => setMimoCapability('imageInput', value)
          : isMinimaxTool
            ? (value) => setMinimaxCapability('imageInput', value)
            : isQoderTool
              ? (value) => setQoderCapability('imageInput', value)
              : isDshTool
                ? (value) => setDshCapability('imageInput', value)
                : setWorkBuddyImageInput,
    });
  if (show1mToggle)
    capabilityToggles.push({
      label: '1M',
      hint: t(
        isMimoTool || isMinimaxTool || isQoderTool || isDshTool || isWorkBuddyApp
          ? 'agent.oneMContextHint'
          : 'agent.claude1mHint'
      ),
      checked: isMimoTool
        ? mimo1mMode
        : isMinimaxTool
          ? minimax1mMode
          : isQoderTool
            ? qoder1mMode
            : isDshTool
              ? dsh1mMode
              : isWorkBuddyApp
                ? workBuddy1mMode
                : isClaudeDesktopApp
                  ? claudeDesktop1mMode
                  : claude1mMode,
      onChange: isMimoTool
        ? (value) => setMimoCapability('oneMContext', value)
        : isMinimaxTool
          ? (value) => setMinimaxCapability('oneMContext', value)
          : isQoderTool
            ? (value) => setQoderCapability('oneMContext', value)
            : isDshTool
              ? (value) => setDshCapability('oneMContext', value)
              : isWorkBuddyApp
                ? setWorkBuddy1mMode
                : isClaudeDesktopApp
                  ? setClaudeDesktop1mMode
                  : setClaude1mMode,
    });
  const routingControls = capabilityToggles.length > 0 && (
    <CapabilityToggles toggles={capabilityToggles} disabled={isLaunching} />
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Header */}
      <div className="h-10 px-2 flex items-center justify-between bg-transparent">
        <span className="min-w-0 truncate px-3 py-1.5 text-sm font-semibold text-cyber-text">
          {selectedToolData ? toolDisplayName(selectedToolData, locale) : ''}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          {supportsWarmUp && (
            <button
              type="button"
              aria-label={t('agent.warmUpAccount')}
              disabled={selectedToolAction !== null || isRefreshingSelectedTool}
              onClick={() => void warmUpSelectedTool()}
              className="account-icon-button flex h-7 w-7 items-center justify-center rounded-full transition-colors hover:bg-cyber-elevated/45 disabled:opacity-40"
            >
              <Zap size={12} className={selectedToolAction === 'warmUp' ? 'animate-pulse' : ''} />
            </button>
          )}
          {supportsClaim && (
            <button
              type="button"
              aria-label={t('accountCenter.claimAll')}
              disabled={selectedToolAction !== null || isRefreshingSelectedTool}
              onClick={() => void claimSelectedTool()}
              className="account-icon-button flex h-7 w-7 items-center justify-center rounded-full transition-colors hover:bg-cyber-elevated/45 disabled:opacity-40"
            >
              <Gift size={12} className={selectedToolAction === 'claim' ? 'animate-pulse' : ''} />
            </button>
          )}
          <button
            type="button"
            aria-label={t('btn.refresh')}
            disabled={!selectedToolData || isRefreshingSelectedTool || selectedToolAction !== null}
            onClick={() => void refreshSelectedTool()}
            className="account-icon-button flex h-7 w-7 items-center justify-center rounded-full transition-colors hover:bg-cyber-elevated/45 disabled:opacity-40"
          >
            <RefreshCw size={12} className={isRefreshingSelectedTool ? 'animate-spin' : ''} />
          </button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2 pt-0">
        {selectedToolData ? (
          // Not installed yet — offer the tool's website or repository
          // alongside the bottom bar's one-click install action.
          !selectedToolData.installed ? (
            selectedToolData.website && (
              <div className="h-full flex flex-col items-center justify-center gap-2 px-3 text-center">
                <p className="text-sm text-cyber-text-secondary leading-relaxed">
                  {t(
                    /^https?:\/\/(?:www\.)?github\.com(?:\/|$)/i.test(selectedToolData.website)
                      ? 'aiDesktop.githubRepository'
                      : 'aiDesktop.officialWebsite'
                  )}
                </p>
                <a
                  href={selectedToolData.website}
                  target="_blank"
                  rel="noopener noreferrer"
                  onClick={(event) => {
                    event.preventDefault();
                    const url = event.currentTarget.href;
                    void api.openExternal(url).catch(() => {
                      window.open(url, '_blank', 'noopener,noreferrer');
                    });
                  }}
                  className="inline-flex max-w-full items-center gap-2 rounded-md border border-cyber-border/50 bg-cyber-surface px-3 py-2 text-xs text-cyber-text-secondary transition-colors hover:border-cyber-accent/40 hover:bg-cyber-accent/5 hover:text-cyber-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyber-accent"
                >
                  <span className="min-w-0 truncate">
                    {selectedToolData.website.replace(/^https?:\/\//, '').replace(/\/$/, '')}
                  </span>
                  <ExternalLink size={13} className="flex-shrink-0" aria-hidden="true" />
                </a>
              </div>
            )
          ) : selectedTool === 'manus' || selectedTool === 'cue' ? (
            <ManusAccountSection tool={selectedTool} />
          ) : selectedTool === 'antigravity' || selectedTool === 'antigravitydesktop' ? (
            <AntigravityAccountSection />
          ) : selectedTool === 'grokbot' ||
            selectedTool === 'cursor' ||
            selectedTool === 'cursorcli' ? (
            <CursorAccountSection tool={selectedTool} />
          ) : selectedTool === 'kirocrew' || selectedTool === 'kirocli' ? (
            <KiroAccountSection />
          ) : selectedTool === 'trae' || selectedTool === 'traecn' ? (
            <TraeAccountSection />
          ) : selectedTool === 'devin' || selectedTool === 'devincli' ? (
            <DevinAccountSection />
          ) : selectedTool === 'doubaowork' || selectedTool === 'dola' ? (
            <DoubaoAccountSection />
          ) : selectedTool === 'dumate' ? (
            <DuMateAccountSection />
          ) : selectedTool === 'qwenwork' || selectedTool === 'qwenworkcn' ? (
            <QwenWorkAccountSection />
          ) : selectedToolData.noModelConfig ? (
            <div className="h-full flex flex-col items-center justify-center gap-3 text-center">
              <BoxIcon size={28} className="text-cyber-text opacity-25" />
              <p className="text-base text-cyber-text-secondary font-mono leading-relaxed">
                {t('agent.noModelConfig')}
              </p>
            </div>
          ) : (
            <div className="flex h-full flex-col gap-2">
              {selectedTool === 'opendesign' && <OpenDesignAccountSection showDivider />}
              {selectedTool === 'codearts' && (
                <CodeArtsAccountSection showDivider={hasVisibleModels} />
              )}
              {showCodexAccounts && (
                <CodexAccountSection showDivider={hasVisibleModels || showWebSearchToggle} />
              )}
              {selectedTool === 'dsh' && <DeepSeekAccountSection showDivider={hasVisibleModels} />}
              {selectedTool === 'grok' && <GrokAccountSection showDivider={hasVisibleModels} />}
              {(selectedTool === 'cline' || selectedTool === 'clinedesktop') && (
                <ClineAccountSection showDivider={hasVisibleModels} />
              )}
              {(selectedTool === 'kilo' || selectedTool === 'kilodesktop') && (
                <ClientAccountSection provider="kilo" showDivider={hasVisibleModels} />
              )}
              {(selectedTool === 'copilot' || selectedTool === 'copilotdesktop') && (
                <ClientAccountSection provider="copilot" showDivider={hasVisibleModels} />
              )}
              {selectedTool === 'zcode' && <ZCodeAccountSection showDivider={hasVisibleModels} />}
              {['kimicode', 'kimidesktop'].includes(selectedTool ?? '') && (
                <KimiAccountSection showDivider={hasVisibleModels} />
              )}
              {selectedTool === pluginAccounts?.provider?.application && (
                <PluginToolAccountSection showDivider={hasVisibleModels} />
              )}
              {(selectedTool === 'minimaxcode' || selectedTool === 'minimaxdesktop') && (
                <MinimaxAccountSection showDivider={hasVisibleModels} />
              )}
              {(selectedTool === 'qoder' || selectedTool === 'qodercn') && (
                <QoderAccountSection showDivider={hasVisibleModels} />
              )}
              {(selectedTool === 'workbuddy' || selectedTool === 'workbuddyai') && (
                <WorkBuddyAccountSection showDivider={hasVisibleModels} />
              )}
              {isClaudeCodeApp && (
                <ClaudeCodeAccountSection showDivider={hasVisibleModels || show1mToggle} />
              )}
              {isClaudeDesktopApp && (
                <ClaudeCodeAccountSection desktop showDivider={hasVisibleModels || show1mToggle} />
              )}
              {routingControls}
              <div className="flex-1">
                <ModelListSection
                  smartRouterEnabled={smartRouterEnabled}
                  selectedToolData={selectedToolData}
                  userModels={userModels}
                  toolModelConfig={toolModelConfig}
                  selectedTool={selectedTool}
                  handleSelectModel={handleSelectModel}
                  modelUsageData={modelUsageData}
                  refreshingUsageIds={refreshingUsageIds}
                  isRefreshingUsage={isRefreshingUsage}
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
                    if (ok) await handleCardDelete(modelId);
                  }}
                  t={t}
                />
              </div>
            </div>
          )
        ) : (
          <div className="h-full flex items-center justify-center">
            <p className="text-cyber-text-secondary text-center">{t('agent.selectTool')}</p>
          </div>
        )}
      </div>
    </div>
  );
};

// ===== Bottom Bar (launch area) =====

export const AppManagerBottom: React.FC = () => {
  const { t, locale } = useI18n();
  const activePage = useNavigationStore((s) => s.activePage);
  const goToMother = useNavigationStore((s) => s.goToMother);
  const {
    viewMode,
    selectedTool,
    selectedToolData,
    toolModelConfig,
    selectedCodexAccountId,
    claudeCodeAccounts,
    claudeDesktopAccounts,
    workBuddyAccounts,
    traeAccounts,
    kiroAccounts,
    zcodeAccounts,
    kimiAccounts,
    pluginAccounts,
    codeArtsAccounts,
    devinAccounts,
    openDesignAccounts,
    minimaxAccounts,
    qoderAccounts,
    dumateAccounts,
    doubaoAccounts,
    qwenWorkAccounts,
    deepSeekAccounts,
    grokAccounts,
    clineAccounts,
    kiloAccounts,
    copilotAccounts,
    manusAccounts,
    cueAccounts,
    grokBotAccounts,
    cursorAccounts,
    antigravityAccounts,
    launchAfterApply,
    setLaunchAfterApply,
    isLaunching,
    agreedConfigPolicy,
    setAgreedConfigPolicy,
    handleLaunch,
    onGoToMother,
  } = useAppManager();

  const [cliSettingsOpen, setCliSettingsOpen] = useState(false);
  const noModelConfig = !!selectedToolData?.noModelConfig;
  // This preview has no desktop installer or native launch integration yet.
  const isCatalogOnly = selectedTool === 'claudeduet';
  // An uninstalled tool flips the primary action to "一键安装" — one click
  // walks the user to the One-Click Install (Mother) page prefilled with the
  // install prompt. Model config / launch are meaningless until the tool is
  // actually on the machine.
  const isUninstalled = !!selectedToolData && !selectedToolData.installed;
  const isInstallAction = isUninstalled || (activePage === 'apps' && viewMode === 'install');
  const isPlugin = !!selectedToolData?.plugin;
  const isBuiltInApp = selectedTool === 'reversi' || selectedTool === 'translator' || isPlugin;
  const showCliSettings =
    selectedToolData?.category === 'CLI Code' && !isCatalogOnly && !isInstallAction && !isPlugin;
  const hasModelSelected = !!(selectedTool && toolModelConfig[selectedTool]);
  const codeArtsAccountSelected = selectedTool === 'codearts' && !!codeArtsAccounts.selectedId;
  const hasAccountSelected =
    codeArtsAccountSelected ||
    (['minimaxcode', 'minimaxdesktop'].includes(selectedTool ?? '') &&
      !!minimaxAccounts.selectedId) ||
    (['qoder', 'qodercn'].includes(selectedTool ?? '') && !!qoderAccounts.selectedId) ||
    (selectedTool === 'dumate' && !!dumateAccounts.selectedId) ||
    (['doubaowork', 'dola'].includes(selectedTool ?? '') && !!doubaoAccounts.selectedId) ||
    (['qwenwork', 'qwenworkcn'].includes(selectedTool ?? '') && !!qwenWorkAccounts.selectedId) ||
    (selectedTool === 'zcode' && !!zcodeAccounts.selectedId) ||
    (['kimicode', 'kimidesktop'].includes(selectedTool ?? '') && !!kimiAccounts.selectedId) ||
    (selectedTool === pluginAccounts?.provider?.application && !!pluginAccounts.selectedId) ||
    (['devin', 'devincli'].includes(selectedTool ?? '') && !!devinAccounts.selectedId) ||
    (selectedTool === 'opendesign' && !!openDesignAccounts.selectedId) ||
    ((selectedTool === 'codex' || selectedTool === 'chatgptdesktop') && !!selectedCodexAccountId) ||
    (selectedTool === 'claudecode' && !!claudeCodeAccounts.selectedId) ||
    (selectedTool === 'claudedesktop' && !!claudeDesktopAccounts.selectedId) ||
    (selectedTool === 'dsh' && !!deepSeekAccounts.selectedId) ||
    (selectedTool === 'grok' && !!grokAccounts.selectedId) ||
    (['cline', 'clinedesktop'].includes(selectedTool ?? '') && !!clineAccounts.selectedId) ||
    ((selectedTool === 'kilo' || selectedTool === 'kilodesktop') && !!kiloAccounts.selectedId) ||
    ((selectedTool === 'copilot' || selectedTool === 'copilotdesktop') &&
      !!copilotAccounts.selectedId) ||
    (selectedTool === 'manus' && !!manusAccounts.selectedId) ||
    (selectedTool === 'cue' && !!cueAccounts.selectedId) ||
    (selectedTool === 'grokbot' && !!grokBotAccounts.selectedId) ||
    (['cursor', 'cursorcli'].includes(selectedTool ?? '') && !!cursorAccounts.selectedId) ||
    ((selectedTool === 'antigravity' || selectedTool === 'antigravitydesktop') &&
      !!antigravityAccounts.selectedId) ||
    ((selectedTool === 'workbuddy' || selectedTool === 'workbuddyai') &&
      !!workBuddyAccounts.selectedId) ||
    ((selectedTool === 'trae' || selectedTool === 'traecn') && !!traeAccounts.selectedId) ||
    ((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && !!kiroAccounts.selectedId);
  const launchOptionDisabled = noModelConfig && !hasAccountSelected;
  const configOptionDisabled = noModelConfig || hasAccountSelected;
  // What will a click actually do?
  //  - "Apply" runs only when the user picked a model AND agreed to the config-write policy.
  //  - "Launch" runs whenever launchAfterApply is on, or unconditionally for desktop/no-config apps.
  // Many tools already work out of the box, so launching without picking a model must stay enabled —
  // forcing model selection just to start a CLI was the long-standing bug.
  const willApply =
    hasAccountSelected || (!isPlugin && !noModelConfig && agreedConfigPolicy && hasModelSelected);
  const willLaunch =
    isPlugin ||
    launchAfterApply ||
    (!hasAccountSelected &&
      (noModelConfig || ['qoder', 'qodercn', 'claudescience'].includes(selectedTool ?? '')));
  const buttonDisabled =
    !selectedToolData ||
    isLaunching ||
    (selectedTool === 'codearts' && codeArtsAccounts.busy) ||
    (selectedTool === 'zcode' && zcodeAccounts.busy) ||
    (['kimicode', 'kimidesktop'].includes(selectedTool ?? '') && kimiAccounts.busy) ||
    (selectedTool === pluginAccounts?.provider?.application && pluginAccounts.busy) ||
    (['devin', 'devincli'].includes(selectedTool ?? '') && devinAccounts.busy) ||
    (selectedTool === 'opendesign' && openDesignAccounts.busy) ||
    (['minimaxcode', 'minimaxdesktop'].includes(selectedTool ?? '') && minimaxAccounts.busy) ||
    (['qoder', 'qodercn'].includes(selectedTool ?? '') && qoderAccounts.busy) ||
    (selectedTool === 'dumate' && dumateAccounts.busy) ||
    (['doubaowork', 'dola'].includes(selectedTool ?? '') && doubaoAccounts.busy) ||
    (['qwenwork', 'qwenworkcn'].includes(selectedTool ?? '') && qwenWorkAccounts.busy) ||
    (selectedTool === 'grok' && grokAccounts.busy) ||
    (['cline', 'clinedesktop'].includes(selectedTool ?? '') && clineAccounts.busy) ||
    ((selectedTool === 'kilo' || selectedTool === 'kilodesktop') && kiloAccounts.busy) ||
    ((selectedTool === 'copilot' || selectedTool === 'copilotdesktop') && copilotAccounts.busy) ||
    (selectedTool === 'manus' && manusAccounts.busy) ||
    (selectedTool === 'cue' && cueAccounts.busy) ||
    (selectedTool === 'claudedesktop' && claudeDesktopAccounts.busy) ||
    (selectedTool === 'grokbot' && grokBotAccounts.busy) ||
    (['cursor', 'cursorcli'].includes(selectedTool ?? '') && cursorAccounts.busy) ||
    ((selectedTool === 'trae' || selectedTool === 'traecn') && traeAccounts.busy) ||
    ((selectedTool === 'kirocrew' || selectedTool === 'kirocli') && kiroAccounts.busy) ||
    (!isUninstalled && !willApply && !willLaunch);

  // Uninstalled → install flow; otherwise the existing launch/apply flow.
  const handlePrimaryClick = () => {
    if (isUninstalled && selectedToolData) {
      onGoToMother(selectedTool!, toolDisplayName(selectedToolData, locale));
      return;
    }
    void handleLaunch();
  };

  const showUninstallHint =
    activePage === 'apps' &&
    !isCatalogOnly &&
    selectedToolData?.installed &&
    !isInstallAction &&
    !isBuiltInApp;

  return (
    <div className="flex-shrink-0 flex flex-col mt-2">
      <div className="mx-2 border-t border-cyber-border"></div>
      <div className="flex items-center justify-end gap-8 px-6 pt-5 pb-1">
        {/* Page-aware hint copy: direct Responses clients get a compatibility
            reminder; Claude proxy clients get the keep-running reminder. */}
        <PageAwareHint />
        <div className="flex shrink-0 self-end items-center gap-8">
          {showCliSettings && (
            <button
              type="button"
              aria-label={t('cliLaunch.title')}
              disabled={isLaunching}
              onClick={() => setCliSettingsOpen(true)}
              className="-mr-5 flex h-14 w-14 flex-shrink-0 items-center justify-center rounded-lg text-cyber-text-secondary transition-colors enabled:hover:text-cyber-text disabled:opacity-40"
            >
              <Settings size={28} aria-hidden="true" />
            </button>
          )}
          {/* Launch button */}
          <div className="relative w-64 h-14 flex-shrink-0">
            {!isCatalogOnly && (
              <button
                onClick={handlePrimaryClick}
                disabled={buttonDisabled}
                className={`w-full h-14 text-lg font-bold font-mono tracking-widest transition-colors rounded-lg cjk-btn border shadow-lg ${
                  buttonDisabled
                    ? 'bg-cyber-border text-cyber-text-secondary border-transparent shadow-none cursor-not-allowed'
                    : 'bg-cyber-accent text-white border-cyber-accent hover:bg-cyber-accent-secondary hover:border-cyber-accent-secondary shadow-cyber-accent/30'
                }`}
              >
                {isInstallAction
                  ? t('btn.installOneClick')
                  : willLaunch
                    ? t('btn.launchApp')
                    : t('btn.modifyOnly')}
              </button>
            )}
            {showUninstallHint && (
              <button
                type="button"
                onClick={() =>
                  goToMother(
                    t('mother.hintUninstall').replace(
                      '{agent}',
                      toolDisplayName(selectedToolData, locale)
                    )
                  )
                }
                className="absolute top-full left-1/2 -translate-x-1/2 mt-1 max-w-full whitespace-nowrap text-xs text-cyber-text-secondary hover:text-cyber-accent transition-colors"
              >
                {t('mother.hintUninstall').replace(
                  '{agent}',
                  toolDisplayName(selectedToolData, locale)
                )}
              </button>
            )}
          </div>
          {/* Reserve the controls' space while installing so the action stays aligned.
            Account selections only need the launch choice; apps without model
            configuration disable both controls. */}
          <div
            className={`flex flex-col gap-2 ${
              isCatalogOnly || isInstallAction || isPlugin ? 'invisible pointer-events-none' : ''
            }`}
          >
            {/* Apply & Launch checkbox */}
            <label
              className={`flex items-center gap-2 select-none ${
                launchOptionDisabled ? 'opacity-40 pointer-events-none' : ''
              }`}
              onClick={() => {
                if (!launchOptionDisabled) setLaunchAfterApply(!launchAfterApply);
              }}
            >
              <div
                className={`w-3.5 h-3.5 border flex items-center justify-center transition-all flex-shrink-0 ${
                  launchAfterApply
                    ? 'border-cyber-border bg-cyber-text/20'
                    : 'border-cyber-border hover:border-cyber-text-muted'
                }`}
              >
                {launchAfterApply && (
                  <svg
                    width="8"
                    height="8"
                    viewBox="0 0 10 10"
                    fill="none"
                    className="text-cyber-text"
                  >
                    <path
                      d="M2 5L4 7L8 3"
                      stroke="currentColor"
                      strokeWidth="1.5"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                )}
              </div>
              <span
                className={`text-xs font-mono transition-colors ${launchAfterApply ? 'text-cyber-text' : 'text-cyber-text-secondary'}`}
              >
                {t('agent.applyAndLaunch')}
              </span>
            </label>
            {/* Config policy agreement */}
            <label
              className={`flex items-center gap-2 select-none ${
                configOptionDisabled ? 'opacity-40 pointer-events-none' : ''
              }`}
              onClick={() => {
                if (!configOptionDisabled) setAgreedConfigPolicy(!agreedConfigPolicy);
              }}
            >
              <div
                className={`w-3.5 h-3.5 border flex items-center justify-center transition-all flex-shrink-0 ${
                  agreedConfigPolicy
                    ? 'border-cyber-border bg-cyber-text/20'
                    : 'border-cyber-border hover:border-cyber-text-muted'
                }`}
              >
                {agreedConfigPolicy && (
                  <svg
                    width="8"
                    height="8"
                    viewBox="0 0 10 10"
                    fill="none"
                    className="text-cyber-text"
                  >
                    <path
                      d="M2 5L4 7L8 3"
                      stroke="currentColor"
                      strokeWidth="1.5"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                )}
              </div>
              <span
                className={`text-xs font-mono transition-colors ${agreedConfigPolicy ? 'text-cyber-text' : 'text-cyber-text-secondary'}`}
              >
                {t('agent.appliedVia')}
              </span>
            </label>
          </div>
        </div>
      </div>
      {cliSettingsOpen && selectedTool && (
        <CliLaunchDialog
          initialTool={selectedTool}
          toolLabel={(tool) => toolDisplayName(tool, locale)}
          onClose={() => setCliSettingsOpen(false)}
        />
      )}
    </div>
  );
};

// Application launch hints shown at the bottom-left of the launch row.
const APP_TIP_KEYS = [
  'hint.cliRecommendation',
  'hint.appReorder',
  'hint.smartAccess',
  'hint.voiceInput',
  'hint.voiceSelection',
] as const;

const RandomAppTip: React.FC = () => {
  const { t } = useI18n();
  const [tipIndex, setTipIndex] = useState(() => Math.floor(Math.random() * APP_TIP_KEYS.length));

  useEffect(() => {
    const timer = setInterval(() => {
      const offset = 1 + Math.floor(Math.random() * (APP_TIP_KEYS.length - 1));
      setTipIndex((current) => (current + offset) % APP_TIP_KEYS.length);
    }, 600_000);
    return () => clearInterval(timer);
  }, []);

  return <>{t(APP_TIP_KEYS[tipIndex])}</>;
};

export const PageAwareHint: React.FC = () => {
  const { t } = useI18n();
  const { viewMode, selectedTool } = useAppManager();
  const toolsActive = useNavigationStore(
    (s) => s.activePage === 'apps' && (s.appsTab === 'desktop' || s.appsTab === 'install')
  );
  const hintKeys: TKey[] =
    viewMode === 'install'
      ? ['aiDesktop.installHint']
      : selectedTool === 'chatgptdesktop'
        ? ['hint.chatgptHistoryMerged', 'hint.responsesRequired']
        : selectedTool === 'codex'
          ? ['hint.responsesRequired']
          : selectedTool === 'claudedesktop'
            ? ['hint.claudeHistoryMerged']
            : ['workbuddy', 'workbuddyai'].includes(selectedTool ?? '')
              ? ['hint.localHistoryPreserved']
              : [];
  return (
    <div className="min-w-0 flex-1 text-[15px] font-medium text-cyber-accent">
      {toolsActive && (
        <div className="line-clamp-2 break-words">
          {hintKeys.length ? hintKeys.map((hintKey) => t(hintKey)).join(' ') : <RandomAppTip />}
        </div>
      )}
    </div>
  );
};

// ===== Apply Error Modal =====

export const AppManagerErrorModal: React.FC = () => {
  const { t } = useI18n();
  const { applyError, setApplyError } = useAppManager();

  if (!applyError) return null;

  return (
    <div className="fixed inset-0 z-[9998] flex items-center justify-center">
      <div className="absolute inset-0" onClick={() => setApplyError(null)} />
      <div className="relative w-[360px] max-w-[90vw] border border-red-500/40 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden">
        <div className="h-[2px] w-full bg-red-500/60" />
        <div className="px-5 pt-4 pb-2 flex items-center gap-2">
          <svg
            className="w-4 h-4 text-red-400 flex-shrink-0"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
            <line x1="12" y1="9" x2="12" y2="13" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
          <span className="text-sm font-mono font-bold tracking-wider text-red-400">
            {t('agent.configWarning')}
          </span>
        </div>
        <div className="px-5 pb-5">
          <p className="text-xs text-cyber-text-secondary leading-relaxed font-mono">
            {applyError}
          </p>
        </div>
        <div className="flex border-t border-cyber-border">
          <button
            onClick={() => setApplyError(null)}
            className="flex-1 px-4 py-2.5 text-xs font-mono font-bold tracking-wider text-red-400 hover:bg-red-500/10 hover:text-red-300 transition-all"
          >
            {t('common.confirm')}
          </button>
        </div>
      </div>
    </div>
  );
};
