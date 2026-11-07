// Model Nexus Page — Model cards, debug console, add/edit modal
// Extracted from App.tsx with Provider pattern for shared state
import {
  useState,
  useEffect,
  useLayoutEffect,
  useCallback,
  useMemo,
  useRef,
  type ReactNode,
} from 'react';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { readText as readClipboardText } from '@tauri-apps/plugin-clipboard-manager';
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
  rectSortingStrategy,
  sortableKeyboardCoordinates,
  arrayMove,
} from '@dnd-kit/sortable';
import { X, Lock, Unlock, RefreshCw, GripVertical, Box, ExternalLink, Plus } from 'lucide-react';
import { DirectoryRow } from '../../components/DirectoryRow';
import { SortableCard } from '../../components/SortableCard';
import { ModelCard, ModelCardSkeleton, getModelIcon } from '../../components';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { PageActionButton } from '../../components/PageActionButton';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import { normalizeAnthropicUrl, normalizeOpenaiUrl } from '../../utils/normalizeUrl';
import { useModelNexus, type DirectoryVariant } from './context';
import modelDirectory from '../../data/modelDirectory.json';
import { useFreeModels } from '../FreeModels/provider';
import { AccountModelFields } from './AccountModelFields';
import { ApiModelFields } from './ApiModelFields';
import { isVolcengineUrl } from './provider';
import { VoiceApiFields } from '../VoiceInput/VoiceApiFields';
import { saveVoiceSource } from '../VoiceInput/sources';
import { useVoiceInputStore, voiceBusy } from '../../stores/voiceInputStore';
import { useNavigationStore } from '../../stores/navigationStore';
import type { TKey } from '../../i18n/types';
import './ModelNexus.css';
export { ModelNexusProvider } from './provider';

const BLANK_MODEL_FORM = { name: '', baseUrl: '', anthropicUrl: '', apiKey: '', modelId: '' };

const isZhipuCnUrl = (baseUrl: string, anthropicUrl?: string | null) => {
  try {
    const url = new URL(baseUrl || anthropicUrl || '');
    return url.protocol === 'https:' && url.hostname === 'open.bigmodel.cn';
  } catch {
    return false;
  }
};

const isValidModelBaseUrl = (value: string) => {
  try {
    const url = new URL(value);
    return (url.protocol === 'http:' || url.protocol === 'https:') && Boolean(url.host);
  } catch {
    return false;
  }
};

// ===== Title Actions (view mode tabs + ping/refresh button) =====

export function ModelNexusTitleActions() {
  const { t } = useI18n();
  const { viewMode, setViewMode, pingAllModels, refreshAllUsage, isTesting, isRefreshingUsage } =
    useModelNexus();

  return (
    <div className="ml-auto flex-shrink-0 flex items-center gap-3">
      {/* View mode tabs */}
      <ViewModeTabs
        value={viewMode}
        onChange={setViewMode}
        options={[
          { value: 'config', label: t('model.config') },
          { value: 'usage', label: t('model.usage') },
        ]}
      />

      {/* Action button - same height and style as tabs */}
      {viewMode === 'config' ? (
        <PageActionButton
          icon={RefreshCw}
          spinning={isTesting}
          onClick={pingAllModels}
          disabled={isTesting}
          className="font-mono"
        >
          {t('btn.pingAll')}
        </PageActionButton>
      ) : (
        <PageActionButton
          icon={RefreshCw}
          spinning={isRefreshingUsage}
          onClick={refreshAllUsage}
          disabled={isRefreshingUsage}
          className="font-mono"
        >
          {t('btn.refreshUsage')}
        </PageActionButton>
      )}
    </div>
  );
}

export function ModelNexusMain() {
  const { showToast } = useToast();
  const orderPending = useRef(false);
  const { t } = useI18n();
  const {
    userModels,
    isLoadingModels,
    viewMode,
    modelUsageData,
    setTestOutput: _setTestOutput,
    testProtocol: _testProtocol,
    modelLatencies,
    pingingModelIds,
    modelTerminals: _modelTerminals,
    isTesting: _isTesting,
    editingModelId: _editingModelId,
    setEditingModelId,
    setNewModelForm,
    setShowAddModelModal,
    keyDestroyed: _keyDestroyed,
    handleCardEdit,
    handleCardDelete,
    openAkskModal,
    openZhipuTeamModal,
    refreshSingleUsage,
    refreshingUsageIds,
    volcAkSkMissingIds,
  } = useModelNexus();

  // Drag-reorder: pointer (5px activation so plain clicks pass through) +
  // keyboard (a11y). On drop, reorder in place and persist the full visible
  // order. Persistence failures restore the prior order and remain visible.
  // autoScroll is disabled on purpose: the grid lives in an overflow-y-auto
  // container and edge-drag auto-scroll felt janky. A DragOverlay (portal)
  // floats the dragged card above everything, so the scroll container never
  // clips the lifted card or its shadow.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );

  const [activeDragId, setActiveDragId] = useState<string | null>(null);
  const [activeDragWidth, setActiveDragWidth] = useState(0);

  const handleDragStart = (event: DragStartEvent) => {
    const id = String(event.active.id);
    setActiveDragId(id);
    // dnd-kit's active.rect is not measured yet at onDragStart (null), so
    // measure the grid node directly for the DragOverlay ghost's width.
    const el = document.querySelector<HTMLElement>(`[data-sortable-card="${id}"]`);
    setActiveDragWidth(el?.offsetWidth ?? 0);
  };

  const handleDragEnd = (event: DragEndEvent) => {
    setActiveDragId(null);
    const activeId = String(event.active.id);
    const overId = String(event.over?.id ?? '');
    if (orderPending.current || !overId || activeId === overId) return;
    const oldIndex = userModels.findIndex((m) => m.internalId === activeId);
    const newIndex = userModels.findIndex((m) => m.internalId === overId);
    if (oldIndex < 0 || newIndex < 0) return;
    const next = arrayMove(userModels, oldIndex, newIndex);
    orderPending.current = true;
    void api
      .reorderModels(next.map((m) => m.internalId))
      .catch((error) => showToast('error', String(error)))
      .finally(() => {
        orderPending.current = false;
      });
  };

  const handleDragCancel = () => setActiveDragId(null);

  const activeDragModel = activeDragId
    ? userModels.find((m) => m.internalId === activeDragId)
    : undefined;

  // Model card body, shared by the grid item and the DragOverlay ghost (the
  // ghost is wrapped in pointer-events-none so its interactions are inert).
  const renderModelCard = (model: (typeof userModels)[0], dragHandle?: ReactNode) => {
    const protocols: ('openai' | 'anthropic')[] = [];
    if (model.baseUrl) protocols.push('openai');
    if (model.anthropicUrl) protocols.push('anthropic');
    const isDemo = model.modelType === 'DEMO';
    return (
      <ModelCard
        id={model.internalId}
        name={model.name}
        type={model.modelType || ''}
        baseUrl={model.baseUrl}
        anthropicUrl={model.anthropicUrl}
        modelId={model.modelId || ''}
        protocols={protocols}
        latency={modelLatencies[model.internalId] ?? model.openaiLatency}
        openaiTested={model.openaiTested}
        anthropicTested={model.anthropicTested}
        isPinging={pingingModelIds.has(model.internalId)}
        viewMode={viewMode}
        usageData={modelUsageData[model.internalId]}
        dragHandle={dragHandle}
        onEdit={isDemo ? undefined : () => handleCardEdit(model)}
        onDelete={isDemo ? undefined : () => handleCardDelete(model.internalId)}
        onRefresh={() => refreshSingleUsage(model.internalId)}
        isRefreshingUsage={refreshingUsageIds.has(model.internalId)}
        onAccessKey={
          isVolcengineUrl(model.baseUrl, model.anthropicUrl)
            ? () => openAkskModal(model.internalId)
            : isZhipuCnUrl(model.baseUrl, model.anthropicUrl)
              ? () => openZhipuTeamModal(model.internalId)
              : undefined
        }
        akSkMissing={volcAkSkMissingIds.has(model.internalId)}
      />
    );
  };

  return (
    <>
      <div className="flex-1 overflow-y-auto">
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragStart={handleDragStart}
          onDragEnd={handleDragEnd}
          onDragCancel={handleDragCancel}
          autoScroll={false}
        >
          <div className="model-nexus-grid model-nexus-grid-fill grid gap-4">
            {/* Show skeleton when loading */}
            {isLoadingModels ? (
              <>
                <ModelCardSkeleton />
                <ModelCardSkeleton />
                <ModelCardSkeleton />
                <ModelCardSkeleton />
              </>
            ) : (
              <>
                {/* User custom models (drag to reorder) */}
                <SortableContext
                  items={userModels.map((m) => m.internalId)}
                  strategy={rectSortingStrategy}
                >
                  {userModels.map((model) => (
                    <SortableCard
                      key={model.internalId}
                      id={model.internalId}
                      dragLabel={t('model.dragSort')}
                    >
                      {(handle) => renderModelCard(model, handle)}
                    </SortableCard>
                  ))}
                </SortableContext>

                {/* Add new model button */}
                <div
                  className={`h-40 border border-dashed border-cyber-border flex flex-col items-center justify-center hover:border-cyber-border cursor-default transition-all rounded-card text-cyber-text-secondary hover:text-cyber-text`}
                  onClick={() => {
                    setNewModelForm(BLANK_MODEL_FORM);
                    setEditingModelId(null);
                    setShowAddModelModal(true);
                  }}
                >
                  <span className="font-bold tracking-wider">{t('btn.addModel')}</span>
                  <span className="text-[10px] opacity-60 mt-1">OpenAI / Anthropic API</span>
                </div>
              </>
            )}
          </div>
          {/* Floating drag ghost — rendered in a portal so the scroll
              container never clips the lifted card or its shadow. */}
          <DragOverlay>
            {activeDragModel && (
              <div
                style={{ width: activeDragWidth }}
                className="pointer-events-none relative shadow-2xl"
              >
                {renderModelCard(
                  activeDragModel,
                  <span aria-hidden="true" className="shrink-0 p-0.5 text-cyber-text-muted/60">
                    <GripVertical size={14} />
                  </span>
                )}
              </div>
            )}
          </DragOverlay>
        </DndContext>
      </div>
    </>
  );
}

// ===== Right Panel (Provider / Relay tabs) =====

// Right-panel Providers + Relays list. Two tiers:
// • Remote-first: `api.getModelDirectory()` hits echobird.ai/api/model-
//   directory/index.json (with backend-side disk cache). Lets us add a
//   vendor or fix a baseUrl without shipping an app release.
// • Bundled fallback: `src/data/modelDirectory.json` shipped in the app
//   binary. Used when both remote and disk-cache are unavailable
//   (offline, first install + firewall, etc.), and as the immediate
//   first paint before the network round-trip lands.
//
// Entries render in file order in every language; `region` stays in the
// data only for older clients. Edit either the remote JSON or the bundled
// JSON; remote wins when both are present.
type DirectoryEntry = {
  name: string;
  url: string;
  baseUrl: string;
  anthropicUrl: string;
  // Optional presets for services that cannot list models.
  modelId?: string;
  modelIds?: string[];
  region: 'cn' | 'global';
  group?: string;
  edition?: string;
};

const BUNDLED_PROVIDERS: DirectoryEntry[] = modelDirectory.providers as DirectoryEntry[];

const BUNDLED_RELAYS: DirectoryEntry[] = modelDirectory.relays as DirectoryEntry[];

// Older remote/cache payloads can still contain obsolete model IDs.
function directoryVariant(entry: DirectoryEntry): DirectoryVariant {
  const preset = [...BUNDLED_PROVIDERS, ...BUNDLED_RELAYS].find(
    (preset) => preset.baseUrl === entry.baseUrl && preset.anthropicUrl === entry.anthropicUrl
  );
  const hasPreset = !!(preset?.modelId || preset?.modelIds?.length);
  const candidates = hasPreset ? (entry.modelIds ?? preset?.modelIds ?? []) : [];
  const defaultId = hasPreset ? entry.modelId || preset?.modelId : undefined;
  return {
    name: entry.name,
    label: editionInfo(entry).edition ?? entry.name,
    url: entry.url,
    baseUrl: entry.baseUrl,
    anthropicUrl: entry.anthropicUrl,
    modelIdOptions: defaultId
      ? [defaultId, ...candidates.filter((id) => id !== defaultId)]
      : candidates,
  };
}

function RelayRow({ entry, onAdd }: { entry: DirectoryEntry; onAdd: () => void }) {
  const openSite = () => shellOpen(entry.url).catch(() => window.open(entry.url, '_blank'));
  return (
    <DirectoryRow
      name={entry.name}
      url={entry.url}
      iconSrc={getModelIcon(entry.name, '')}
      onOpen={openSite}
      openLabel={`Open ${entry.name} website`}
      add={{ onClick: onAdd, label: `Add model: ${entry.name}` }}
    />
  );
}

// Editions of one company (regions or plans) share a `group`. A remote or
// cached directory without the field falls back to the bundled entry with
// the same endpoints, so grouping does not wait for the remote update.
function editionInfo(entry: DirectoryEntry): Pick<DirectoryEntry, 'group' | 'edition'> {
  if (entry.group) return entry;
  const bundled = BUNDLED_PROVIDERS.find(
    (b) => b.baseUrl === entry.baseUrl && b.anthropicUrl === entry.anthropicUrl
  );
  return { group: bundled?.group, edition: bundled?.edition };
}

type ProviderItem = { name: string; entries: DirectoryEntry[] };

function groupProviders(entries: DirectoryEntry[]): ProviderItem[] {
  const items: ProviderItem[] = [];
  const grouped = new Map<string, ProviderItem>();
  for (const entry of entries) {
    const group = editionInfo(entry).group;
    if (!group) {
      items.push({ name: entry.name, entries: [entry] });
      continue;
    }
    const existing = grouped.get(group);
    if (existing) {
      existing.entries.push(entry);
    } else {
      const item = { name: group, entries: [entry] };
      grouped.set(group, item);
      items.push(item);
    }
  }
  // A group with only one edition present keeps that edition's own name.
  return items.map((item) =>
    item.entries.length === 1 ? { ...item, name: item.entries[0].name } : item
  );
}

// Without an icon name the tile is the blank "add model" entry.
function ProviderTile({
  name,
  iconName,
  onAdd,
}: {
  name: string;
  iconName?: string;
  onAdd: () => void;
}) {
  const iconSrc = iconName ? getModelIcon(iconName, '') : null;
  return (
    <button
      type="button"
      onClick={onAdd}
      aria-label={iconName ? `Add model: ${name}` : name}
      className="group flex min-w-0 items-center gap-2 rounded-lg p-2 text-left transition-colors hover:bg-cyber-elevated/50"
    >
      {!iconName ? (
        <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center text-cyber-text-muted">
          <Plus
            size={20}
            strokeWidth={2}
            className="flex-shrink-0 transition-colors group-hover:text-cyber-text"
          />
        </span>
      ) : iconSrc ? (
        <img
          src={iconSrc}
          alt=""
          className="h-5 w-5 flex-shrink-0"
          onError={(event) => {
            event.currentTarget.style.display = 'none';
          }}
        />
      ) : (
        <Box size={18} className="flex-shrink-0 text-cyber-text" aria-hidden="true" />
      )}
      <span className="min-w-0 truncate text-sm text-cyber-text-secondary transition-colors group-hover:text-cyber-text">
        {name}
      </span>
    </button>
  );
}

export function ModelNexusPanel() {
  const { t } = useI18n();

  // Bundled JSON paints immediately, remote swaps in if newer content
  // is available. Failure modes (remote down + cache miss): backend
  // returns null, we keep the bundled state forever. No flicker, no
  // blank panel.
  const [providers, setProviders] = useState<DirectoryEntry[]>(BUNDLED_PROVIDERS);
  const [relays, setRelays] = useState<DirectoryEntry[]>(BUNDLED_RELAYS);

  useEffect(() => {
    let cancelled = false;
    api
      .getModelDirectory()
      .then((remote) => {
        if (cancelled || !remote) return;
        setProviders(remote.providers as DirectoryEntry[]);
        setRelays(remote.relays as DirectoryEntry[]);
      })
      .catch(() => {
        /* keep bundled */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // One scrolling list under two headed sections: sponsored relays on top,
  // providers below. Both keep the directory file order in every language.
  const sections = useMemo(() => {
    const ordered: { key: string; title: string; entries: DirectoryEntry[] }[] = [
      { key: 'relays', title: t('model.relays'), entries: relays },
      { key: 'providers', title: t('model.providers'), entries: providers },
    ];
    // Hide a group the directory doesn't currently provide.
    return ordered.filter((s) => s.entries.length > 0);
  }, [providers, relays, t]);
  const { setNewModelForm, setEditingModelId, setShowAddModelModal } = useModelNexus();

  const openAddModel = useCallback(
    (entries: DirectoryEntry[]) => {
      const variants = entries.map(directoryVariant);
      const first = variants[0];
      setNewModelForm({
        name: first.name,
        baseUrl: first.baseUrl,
        anthropicUrl: first.anthropicUrl,
        apiKey: '',
        // Discovery chooses the default; presets apply only when it is unsupported.
        modelId: '',
        modelIdOptions: first.modelIdOptions,
        directoryVariants: variants.length > 1 ? variants : undefined,
      });
      setEditingModelId(null);
      setShowAddModelModal(true);
    },
    [setNewModelForm, setEditingModelId, setShowAddModelModal]
  );

  const openBlankModel = () => {
    setNewModelForm(BLANK_MODEL_FORM);
    setEditingModelId(null);
    setShowAddModelModal(true);
  };

  return (
    <div className="flex-1 px-2 pb-2 overflow-y-auto">
      {sections.map((section) => (
        <div key={section.key}>
          <div className="px-1 py-2 flex items-center gap-2">
            <span className="flex-1 h-px bg-cyber-border/60" />
            <span className="text-[11px] font-semibold uppercase tracking-wider text-cyber-text-secondary whitespace-nowrap">
              {section.title}
            </span>
            <span className="flex-1 h-px bg-cyber-border/60" />
          </div>
          {section.key === 'relays' ? (
            // Sponsored relays keep their full cards.
            <div className="space-y-2">
              {section.entries.map((entry) => (
                <RelayRow key={entry.name} entry={entry} onAdd={() => openAddModel([entry])} />
              ))}
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-x-2 gap-y-0.5">
              <ProviderTile name={t('btn.addModel')} onAdd={openBlankModel} />
              {groupProviders(section.entries).map((item) => (
                <ProviderTile
                  key={item.name}
                  name={item.name}
                  iconName={item.entries[0].name}
                  onAdd={() => openAddModel(item.entries)}
                />
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

// ===== Add/Edit Model Modal =====

function DirectoryVariantTabs({
  variants,
  baseUrl,
  anthropicUrl,
  onSelect,
}: {
  variants: DirectoryVariant[];
  baseUrl: string;
  anthropicUrl: string;
  onSelect: (variant: DirectoryVariant) => void;
}) {
  const active = variants.find((v) => v.baseUrl === baseUrl && v.anthropicUrl === anthropicUrl);
  const site = active?.url;
  return (
    <div className="flex items-stretch gap-2">
      <ViewModeTabs
        value={active?.name ?? ''}
        options={variants.map((v) => ({ value: v.name, label: v.label }))}
        onChange={(name) => {
          const variant = variants.find((v) => v.name === name);
          if (variant) onSelect(variant);
        }}
      />
      {site && (
        <button
          type="button"
          onClick={() => shellOpen(site).catch(() => window.open(site, '_blank'))}
          aria-label={`Open ${active?.name} website`}
          className="flex items-center justify-center px-1 text-cyber-text-secondary transition-colors hover:text-cyber-text"
        >
          <ExternalLink size={20} strokeWidth={2} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}

export function AddModelModal() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [isSavingModel, setIsSavingModel] = useState(false);
  const voiceState = useVoiceInputStore();
  const activePage = useNavigationStore((state) => state.activePage);
  const voiceAttempt = useRef(0);
  const { addSelectedModel, updateSelectedModel } = useFreeModels();
  const {
    showAddModelModal,
    modelModalAnimatingOut,
    editingModelId,
    setEditingModelId,
    newModelForm,
    setNewModelForm,
    keyDestroyed,
    closeModelModal,
    setUserModels,
    updateUserModel,
    setShowAddModelModal,
    modelModalDestination,
    setModelModalDestination,
  } = useModelNexus();

  useLayoutEffect(() => {
    voiceAttempt.current += 1;
  }, [showAddModelModal, modelModalAnimatingOut, modelModalDestination, activePage]);
  useEffect(() => {
    if (showAddModelModal && modelModalDestination === 'voiceInput' && activePage !== 'voice')
      closeModelModal();
  }, [showAddModelModal, modelModalDestination, activePage, closeModelModal]);

  if (!showAddModelModal) return null;
  const voiceForm = modelModalDestination === 'voiceInput';
  const voiceDisabled =
    voiceForm &&
    (!voiceState.snapshot ||
      voiceState.saving ||
      voiceState.pending ||
      voiceBusy(voiceState.snapshot.phase));

  // One-click paste affordance for the modal's free-text fields. Plain text,
  // no button styling / hover effect — the label never changes. The `normalize`
  // arg lets a URL field run the pasted text through the SAME normalization the
  // typing path uses, so a pasted full endpoint (`/v1/chat/completions`) is
  // trimmed to a clean base instead of silently keeping a doubled path that
  // would 404 every request while manually typed URLs work.
  //
  // These URL fields have NO overlay control to their right (unlike the API
  // key field, which has the encrypt/decrypt lock button at right-2), so the
  // paste label sits flush at the right edge — right-2 — and the input keeps
  // enough right padding (pr-16) to clear the widest locale's "貼り付け" text.
  const pasteButton = (field: 'baseUrl' | 'anthropicUrl', normalize: (v: string) => string) => (
    <button
      type="button"
      onClick={async () => {
        try {
          const text = (await readClipboardText()).trim();
          if (text) {
            const v = normalize(text);
            setNewModelForm((prev) => ({ ...prev, [field]: v }));
          }
        } catch {
          /* clipboard empty / unreadable — no-op */
        }
      }}
      className="absolute right-2 top-1/2 -translate-y-1/2 cursor-pointer text-xs text-cyber-text-secondary"
    >
      {t('model.paste')}
    </button>
  );

  return (
    <div
      className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${modelModalAnimatingOut ? 'opacity-0' : 'opacity-100'}`}
      onKeyDown={(e) => {
        if (e.key === 'Escape') closeModelModal();
      }}
    >
      {/* Backdrop */}
      <div className="absolute inset-0" onClick={closeModelModal} />

      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="model-config-dialog-title"
        className={`relative w-[450px] max-w-[90vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden transition-all duration-200 ${modelModalAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'}`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top accent line */}
        <div className="h-px w-full bg-cyber-border" />

        {/* Header */}
        <div className="px-6 pt-5 pb-4 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-cyber-text font-mono text-sm opacity-60">&gt;_</span>
            <span id="model-config-dialog-title" className="text-base font-bold text-cyber-text">
              {editingModelId
                ? newModelForm.accountSource
                  ? '编辑账号配置'
                  : t('model.editConfig')
                : voiceForm
                  ? t(newModelForm.accountSource ? 'voice.accountInput' : 'voice.addModel')
                  : modelModalDestination === 'freeRouter'
                    ? t(
                        newModelForm.accountSource
                          ? 'freeModels.addAccountToRouter'
                          : 'freeModels.addToRouter'
                      )
                    : t('btn.addModel')}
            </span>
          </div>
          <button
            onClick={closeModelModal}
            aria-label={t('btn.close')}
            className="text-cyber-text-secondary close-button transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Form */}
        <div className="px-5 pb-5">
          <div className="space-y-4">
            {!newModelForm.accountSource && (
              <div>
                <label className="block text-xs text-cyber-text-secondary mb-1">
                  {t('model.name')}
                </label>
                <input
                  type="text"
                  placeholder="e.g. My Model"
                  value={newModelForm.name}
                  onChange={(e) => setNewModelForm((prev) => ({ ...prev, name: e.target.value }))}
                  className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                />
              </div>
            )}
            {!newModelForm.accountSource && newModelForm.directoryVariants && (
              <DirectoryVariantTabs
                variants={newModelForm.directoryVariants}
                baseUrl={newModelForm.baseUrl}
                anthropicUrl={newModelForm.anthropicUrl}
                onSelect={(variant) =>
                  setNewModelForm((prev) => {
                    const current = prev.directoryVariants?.find(
                      (v) => v.baseUrl === prev.baseUrl && v.anthropicUrl === prev.anthropicUrl
                    );
                    return {
                      ...prev,
                      // Keep a name the user typed; replace only the edition's own name.
                      name: !prev.name || prev.name === current?.name ? variant.name : prev.name,
                      baseUrl: variant.baseUrl,
                      anthropicUrl: variant.anthropicUrl,
                      modelId: '',
                      modelIdOptions: variant.modelIdOptions,
                    };
                  })
                }
              />
            )}
            {newModelForm.accountSource ? (
              <AccountModelFields
                source={newModelForm.accountSource}
                modelId={newModelForm.modelId}
                modelIdOptions={newModelForm.modelIdOptions}
                setForm={setNewModelForm}
                active={!modelModalAnimatingOut && (!voiceForm || activePage === 'voice')}
                voice={voiceForm}
              />
            ) : (
              <>
                <div>
                  <label className="block text-xs text-cyber-text-secondary mb-1">
                    {t(voiceForm ? 'voice.apiUrl' : 'model.openaiUrl')}
                  </label>
                  <div className="relative">
                    <input
                      type="text"
                      placeholder="https://x.x.com/v1"
                      value={newModelForm.baseUrl}
                      onChange={(e) =>
                        setNewModelForm((prev) => ({
                          ...prev,
                          baseUrl: voiceForm ? e.target.value : normalizeOpenaiUrl(e.target.value),
                        }))
                      }
                      className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 pr-16 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                    />
                    {pasteButton(
                      'baseUrl',
                      voiceForm ? (value) => value.trim() : normalizeOpenaiUrl
                    )}
                  </div>
                </div>
                {!voiceForm && (
                  <div>
                    <label className="block text-xs text-cyber-text-secondary mb-1">
                      {t('model.anthropicUrl')}
                    </label>
                    <div className="relative">
                      <input
                        type="text"
                        placeholder="https://x.x.com/anthropic"
                        value={newModelForm.anthropicUrl}
                        onChange={(e) =>
                          setNewModelForm((prev) => ({
                            ...prev,
                            anthropicUrl: normalizeAnthropicUrl(e.target.value),
                          }))
                        }
                        className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 pr-16 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                      />
                      {pasteButton('anthropicUrl', normalizeAnthropicUrl)}
                    </div>
                  </div>
                )}
              </>
            )}
            {!newModelForm.accountSource &&
              (voiceForm ? (
                <VoiceApiFields
                  form={newModelForm}
                  setForm={setNewModelForm}
                  active={!modelModalAnimatingOut && !isSavingModel && !voiceDisabled}
                />
              ) : (
                <ApiModelFields
                  key={editingModelId ?? 'new'}
                  form={newModelForm}
                  setForm={setNewModelForm}
                  editing={!!editingModelId}
                  active={!modelModalAnimatingOut && !isSavingModel}
                />
              ))}
            {!newModelForm.accountSource && (
              <div>
                <label className="block text-xs text-cyber-text-secondary mb-1">
                  {t('model.apiKey')}
                </label>
                <div className="relative">
                  <input
                    type="text"
                    placeholder="sk-..."
                    value={
                      newModelForm.apiKey.startsWith('enc:v1:')
                        ? '•••••••••••••••'
                        : newModelForm.apiKey
                    }
                    onChange={(e) =>
                      setNewModelForm((prev) => ({ ...prev, apiKey: e.target.value }))
                    }
                    className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 pr-20 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
                    readOnly={newModelForm.apiKey.startsWith('enc:v1:')}
                  />
                  {/* One-click paste from clipboard — plain text affordance (no
                    border / button styling, no hover effect, label never
                    changes). Shown only while the key is editable (plaintext);
                    hidden once encrypted (the field is read-only then). */}
                  {newModelForm.apiKey !== 'not-needed' &&
                    !newModelForm.apiKey.startsWith('enc:v1:') && (
                      <button
                        type="button"
                        onClick={async () => {
                          try {
                            const text = (await readClipboardText()).trim();
                            if (text) {
                              setNewModelForm((prev) => ({ ...prev, apiKey: text }));
                            }
                          } catch {
                            /* clipboard empty / unreadable — no-op */
                          }
                        }}
                        className="absolute right-8 top-1/2 -translate-y-1/2 cursor-pointer text-xs text-cyber-text-secondary"
                      >
                        {t('model.paste')}
                      </button>
                    )}
                  {newModelForm.apiKey !== 'not-needed' && (
                    <button
                      type="button"
                      disabled={!newModelForm.apiKey}
                      onClick={async () => {
                        if (newModelForm.apiKey.startsWith('enc:v1:')) {
                          try {
                            const plain = await api.decryptSecret(newModelForm.apiKey);
                            const newKey = plain || '';
                            setNewModelForm((prev) => ({ ...prev, apiKey: newKey }));
                          } catch {
                            setNewModelForm((prev) => ({ ...prev, apiKey: '' }));
                          }
                        } else {
                          try {
                            const encrypted = await api.encryptSecret(newModelForm.apiKey);
                            setNewModelForm((prev) => ({ ...prev, apiKey: encrypted }));
                          } catch (error) {
                            showToast('error', String(error));
                          }
                        }
                      }}
                      className={`absolute right-2 top-1/2 -translate-y-1/2 transition-colors hover:opacity-80 ${!newModelForm.apiKey ? 'opacity-30 cursor-not-allowed' : ''}`}
                    >
                      {newModelForm.apiKey.startsWith('enc:v1:') ? (
                        <Lock size={14} className="text-cyber-accent" />
                      ) : (
                        <Unlock size={14} className="text-cyber-text/70" />
                      )}
                    </button>
                  )}
                </div>
                {/* Encryption hint — always rendered (locale-aware exact
                  height, no residual space) and toggled via visibility
                  so the form doesn't shift on encrypt / destroy. */}
                <div
                  aria-hidden={!newModelForm.apiKey.startsWith('enc:v1:')}
                  className={`mt-1 text-xs leading-tight ${
                    keyDestroyed ? 'text-red-400' : 'text-cyber-text/60'
                  } ${!newModelForm.apiKey.startsWith('enc:v1:') ? 'invisible' : ''}`}
                >
                  {keyDestroyed ? t('key.destroyed') : t('key.encrypted')}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Footer buttons */}
        <div className="flex border-t border-cyber-border">
          <button
            onClick={closeModelModal}
            className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated transition-all border-r border-cyber-border"
          >
            {t('model.escCancel')}
          </button>
          <button
            type="button"
            disabled={isSavingModel || voiceDisabled}
            onClick={async () => {
              if (isSavingModel || voiceDisabled) return;
              if (
                !newModelForm.accountSource &&
                newModelForm.baseUrl.trim() &&
                !isValidModelBaseUrl(newModelForm.baseUrl)
              ) {
                showToast('warning', t('model.invalidOpenaiUrl'));
                return;
              }
              if (
                newModelForm.accountSource &&
                (!newModelForm.accountSource.accountId || !newModelForm.modelId.trim())
              ) {
                showToast('warning', t('accountProxy.selectRequired'));
                return;
              }
              setIsSavingModel(true);
              const attempt = voiceAttempt.current;
              const isCurrent = () => voiceAttempt.current === attempt;
              try {
                if (voiceForm) {
                  await saveVoiceSource(newModelForm, isCurrent);
                  if (!isCurrent()) return;
                } else if (editingModelId) {
                  const updatedModel = await api.updateModel(editingModelId, {
                    name: newModelForm.name,
                    baseUrl: newModelForm.baseUrl,
                    anthropicUrl: newModelForm.anthropicUrl,
                    apiKey: newModelForm.accountSource ? '' : newModelForm.apiKey,
                    modelId: newModelForm.modelId,
                    accountSource: newModelForm.accountSource,
                  });
                  if (updatedModel) {
                    updateSelectedModel({
                      internalId: updatedModel.internalId,
                      name: updatedModel.name,
                      baseUrl: updatedModel.baseUrl,
                      anthropicUrl: updatedModel.anthropicUrl,
                      apiKey: updatedModel.apiKey,
                      modelId: updatedModel.modelId ?? newModelForm.modelId,
                      accountSource: updatedModel.accountSource,
                      scope: updatedModel.scope,
                    });
                    updateUserModel(updatedModel, newModelForm);
                  }
                } else {
                  const newModel = await api.addModel({
                    name: newModelForm.name,
                    baseUrl: newModelForm.baseUrl,
                    anthropicUrl: newModelForm.anthropicUrl || undefined,
                    apiKey: newModelForm.accountSource ? '' : newModelForm.apiKey,
                    modelId: newModelForm.modelId,
                    accountSource: newModelForm.accountSource,
                    scope: modelModalDestination === 'freeRouter' ? 'smartRouter' : 'modelCenter',
                  });
                  if (modelModalDestination === 'freeRouter') {
                    try {
                      await addSelectedModel({
                        internalId: newModel.internalId,
                        name: newModel.name,
                        baseUrl: newModel.baseUrl,
                        anthropicUrl: newModel.anthropicUrl,
                        apiKey: newModel.apiKey,
                        modelId: newModel.modelId ?? newModelForm.modelId,
                        accountSource: newModel.accountSource,
                        scope: newModel.scope,
                      });
                    } catch (error) {
                      await api.deleteModel(newModel.internalId).catch((rollbackError) => {
                        console.error('Rollback smart router model failed:', rollbackError);
                      });
                      throw error;
                    }
                  } else {
                    setUserModels((prev) =>
                      prev.some((model) => model.internalId === newModel.internalId)
                        ? prev.map((model) =>
                            model.internalId === newModel.internalId ? newModel : model
                          )
                        : [...prev, newModel]
                    );
                  }
                }

                setEditingModelId(null);
                setNewModelForm({
                  name: '',
                  baseUrl: '',
                  anthropicUrl: '',
                  apiKey: '',
                  modelId: '',
                });
                setShowAddModelModal(false);
                setModelModalDestination('modelNexus');
              } catch (error) {
                if (voiceForm && !isCurrent()) return;
                console.error('Save model failed:', error);
                const message = error instanceof Error ? error.message : String(error);
                showToast(
                  'error',
                  t(
                    voiceForm && message.startsWith('voice.error.')
                      ? (message as TKey)
                      : 'error.requestFailed'
                  )
                );
              } finally {
                setIsSavingModel(false);
              }
            }}
            className={`flex-1 px-4 py-3 text-[14px] font-semibold transition-all ${
              isSavingModel
                ? 'text-cyber-text-muted cursor-not-allowed'
                : 'text-cyber-text hover:bg-cyber-text/10'
            }`}
          >
            {t('model.enterSave')}
          </button>
        </div>
      </div>
    </div>
  );
}
