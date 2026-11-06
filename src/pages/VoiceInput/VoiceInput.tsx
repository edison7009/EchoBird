import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  KeyboardSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
  type Modifier,
} from '@dnd-kit/core';
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { AudioLines, Check, Copy, Languages, Keyboard, Mic, Plus, X } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import type { TKey } from '../../i18n';
import type { VoiceChannel, VoiceConfig } from '../../api/voiceInput';
import { useVoiceInputStore, voiceBusy } from '../../stores/voiceInputStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { useAccountPrivacyStore, ACCOUNT_NAME_MASK } from '../../stores/accountPrivacyStore';
import { RemoteModelSelector } from '../../components/RemoteModelSelector';
import { IslandScaleSlider } from '../../components/IslandScaleSlider';
import { MiniSelect } from '../../components/MiniSelect';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { ModelListCard } from '../../components/ModelListCard';
import { SortableCard } from '../../components/SortableCard';
import { DirectoryRow } from '../../components/DirectoryRow';
import { getModelIcon } from '../../components/cards';
import { AccountProxyPanel } from '../FreeModels/AccountProxyPanel';
import { accountSpeechProviders, speechSourceFromSelector } from '../../api/accountProxy';
import modelDirectory from '../../data/modelDirectory.json';
import * as modelApi from '../../api/models';
import {
  getVoiceModels,
  selectionLanguages,
  voiceIslandScales,
  voiceProtocols,
  type VoiceDirectoryEntry,
} from '../../api/voiceInput';
import { useModelNexus } from '../ModelNexus/context';
import { useMotherAgent } from '../MotherAgent/context';
import { isSmartRouterId } from '../../utils/smartRouter';
import { copyText } from '../../utils/copyText';
import { useVoiceAccounts } from './provider';
import { ShortcutRecorder } from './ShortcutRecorder';
import { VoiceChannelCard } from './VoiceChannelCard';
import { IslandContent } from './VoiceIsland';
import { VoiceReplay } from './VoiceReplay';
import { voicePalettes, voicePaletteColor } from './voicePalettes';
import '../FreeModels/FreeModels.css';
import './VoiceInput.css';

const channelKey = (channel: VoiceChannel) => `${channel.kind}:${channel.id}`;
const restrictToVerticalAxis: Modifier = ({ transform }) => ({ ...transform, x: 0 });

export function VoiceInputMain() {
  const { t } = useI18n();
  const state = useVoiceInputStore();
  const accounts = useVoiceAccounts();
  const { userModels } = useModelNexus();
  const { models: textModels } = useMotherAgent();
  const sourceModels = [...state.models, ...userModels];
  const hidden = useAccountPrivacyStore((privacy) => privacy.hidden);
  const active = useNavigationStore((navigation) => navigation.activePage === 'voice');
  const { snapshot, saving, pending, devices, save } = state;
  const [copyState, setCopyState] = useState<{ text: string; success: boolean } | null>(null);
  const [replayErrorId, setReplayErrorId] = useState<number | null>(null);
  const [activeDragId, setActiveDragId] = useState<string | null>(null);
  const [orderedChannels, setOrderedChannels] = useState<VoiceChannel[] | null>(null);
  // Dragging previews locally; the slow config save runs once on release.
  const [scaleDraft, setScaleDraft] = useState<number | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );
  const voiceActive = pending || voiceBusy(snapshot?.phase);
  useEffect(() => () => setActiveDragId(null), [active, voiceActive]);
  const copyTimer = useRef<ReturnType<typeof setTimeout>>();
  const routingStage = useRef<HTMLDivElement>(null);
  useEffect(() => () => clearTimeout(copyTimer.current), []);
  if (!snapshot)
    return (
      <div className="p-5 text-sm text-cyber-text-secondary">
        {state.error ? t('voice.error.config') : t('voice.loading')}
      </div>
    );
  const config = snapshot.config;
  const busy = saving || voiceActive;
  const channelLayoutKey = active && !voiceActive ? 'active' : 'hidden';
  const channels = orderedChannels ?? config.channels;
  const result = state.lastResult ?? snapshot;
  const resultText = result.text;
  const resultAudioId = result.resultAudioId;
  const copyFailed = copyState?.text === resultText && copyState?.success === false;
  const replayFailed = resultAudioId !== null && replayErrorId === resultAudioId;
  const error =
    state.error ||
    (copyFailed ? 'voice.error.copy' : replayFailed ? 'voice.error.replay' : result.error);
  const emptySources = channels.length === 0;
  const selectionModel = config.selectionModel || null;
  const visibleError = error === 'voice.error.noChannels' ? null : error;
  const copied = copyState?.text === resultText && copyState?.success === true;
  const update = <K extends keyof VoiceConfig>(key: K, value: VoiceConfig[K]) => {
    if (!busy) void save({ [key]: value });
  };
  const accountSource = (channel: VoiceChannel) =>
    channel.kind === 'workbuddy'
      ? { provider: 'workBuddy' as const, accountId: channel.id }
      : channel.kind === 'account'
        ? speechSourceFromSelector(channel.id)
        : undefined;
  const label = (channel: VoiceChannel) => {
    if (channel.kind === 'model')
      return (
        sourceModels.find((model) => model.internalId === channel.id)?.name ||
        t('voice.missingSource')
      );
    if (hidden) return ACCOUNT_NAME_MASK;
    const source = accountSource(channel);
    return (
      (source &&
        accounts.accounts.find(
          (account) =>
            account.source.provider === source.provider &&
            account.source.accountId === source.accountId
        )?.label) ||
      accountSpeechProviders.find((provider) => provider.id === source?.provider)?.name ||
      t('voice.missingSource')
    );
  };
  const renderChannel = (index: number, handle?: ReactNode, overlay = false) => {
    const channel = channels[index];
    const model =
      channel?.kind === 'model'
        ? sourceModels.find((item) => item.internalId === channel.id)
        : undefined;
    const source = channel && accountSource(channel);
    const provider = accountSpeechProviders.find((entry) => entry.id === source?.provider);
    return (
      <VoiceChannelCard
        index={index}
        label={index === 0 ? t('voice.primary') : t('voice.backup') + ' ' + index}
        filled={!!channel}
        name={channel ? label(channel) : ''}
        detail={model?.modelId || t(source ? 'voice.accountInput' : 'voice.apiInput')}
        icon={
          provider
            ? `/icons/tools/${provider.icon}`
            : model
              ? getModelIcon(model.name, model.modelId)
              : null
        }
        disabled={busy || activeDragId !== null || overlay}
        active={config.shortcutEnabled && !!channel && snapshot.activeChannel === index && !overlay}
        handle={channel ? handle : undefined}
        overlay={overlay}
        removeLabel={t('voice.removeChannel')}
        onRemove={() =>
          update(
            'channels',
            channels.filter((_, position) => position !== index)
          )
        }
      />
    );
  };
  const handleDragEnd = async ({ active: dragged, over }: DragEndEvent) => {
    setActiveDragId(null);
    if (!active || busy || !over || dragged.id === over.id) return;
    const from = channels.findIndex((channel) => channelKey(channel) === dragged.id);
    const to = channels.findIndex((channel) => channelKey(channel) === over.id);
    if (from < 0 || to < 0) return;
    const next = arrayMove(channels, from, to);
    setOrderedChannels(next);
    try {
      await save({ channels: next });
    } finally {
      setOrderedChannels(null);
    }
  };
  const dragIndex = channels.findIndex((channel) => channelKey(channel) === activeDragId);
  return (
    <div className="flex-1 overflow-y-auto pr-1 pb-3">
      <div className="voice-grid" data-saving={saving && !voiceActive}>
        <div ref={routingStage} className="voice-routing-stage">
          <VoiceRoutingLines
            stageRef={routingStage}
            enabled={config.shortcutEnabled}
            sourceOrder={JSON.stringify(channels)}
            layoutKey={channelLayoutKey}
            channelCount={channels.length}
            activeChannel={snapshot.activeChannel}
          />
          <section className="voice-input-section">
            <div className="voice-preview-stack">
              {(resultText || visibleError || emptySources) && (
                <div
                  className={`voice-result${!resultText && visibleError ? ' is-error' : ''}`}
                  aria-live="polite"
                >
                  <div className="voice-result-content">
                    {resultText && <p>{resultText}</p>}
                    {emptySources && !resultText && !visibleError && (
                      <p>{t('voice.error.noChannels')}</p>
                    )}
                    {visibleError && (
                      <p className="voice-error" role="alert">
                        {visibleError.startsWith('voice.') ? t(visibleError as TKey) : visibleError}
                      </p>
                    )}
                  </div>
                  {(resultText || resultAudioId !== null) && (
                    <div className="voice-result-actions">
                      {resultAudioId !== null && (
                        <VoiceReplay
                          key={`${resultAudioId}:${voiceActive}:${active}`}
                          audioId={resultAudioId}
                          disabled={voiceActive}
                          active={active}
                          onError={(failed) => setReplayErrorId(failed ? resultAudioId : null)}
                        />
                      )}
                      {resultText && (
                        <button
                          type="button"
                          className="voice-text-button"
                          onClick={async () => {
                            const success = await copyText(resultText);
                            setCopyState({ text: resultText, success });
                            clearTimeout(copyTimer.current);
                            if (success)
                              copyTimer.current = setTimeout(() => setCopyState(null), 2000);
                          }}
                        >
                          {copied ? <Check size={14} /> : <Copy size={14} />}
                          {t(copied ? 'voice.copied' : 'voice.copy')}
                        </button>
                      )}
                    </div>
                  )}
                </div>
              )}
              <div className="voice-island-preview">
                <IslandContent snapshot={snapshot} preview />
              </div>
            </div>
            <fieldset disabled={busy} className="voice-island-options">
              <div className="voice-row">
                <span>{t('voice.theme')}</span>
                <ViewModeTabs
                  value={config.islandTheme}
                  onChange={(value) => update('islandTheme', value)}
                  options={(['auto', 'iridescent', 'dark', 'light', 'system'] as const).map(
                    (theme) => ({
                      value: theme,
                      label: t(`voice.${theme}` as TKey),
                    })
                  )}
                />
              </div>
              <div className="voice-row">
                <span>{t('voice.palette')}</span>
                <div className="voice-palette-picker" role="group" aria-label={t('voice.palette')}>
                  {voicePalettes.map((palette) => (
                    <button
                      key={palette}
                      type="button"
                      className="voice-palette-option"
                      aria-label={t(`voice.palette.${palette}` as TKey)}
                      aria-pressed={(config.islandPalette ?? 'white') === palette}
                      onClick={() => update('islandPalette', palette)}
                    >
                      {[5, 9, 14, 9, 5].map((height, index) => (
                        <span
                          key={index}
                          aria-hidden="true"
                          style={{ height, backgroundColor: voicePaletteColor(palette, index / 4) }}
                        />
                      ))}
                    </button>
                  ))}
                </div>
              </div>
            </fieldset>
          </section>
          <DndContext
            key={channelLayoutKey}
            sensors={sensors}
            modifiers={[restrictToVerticalAxis]}
            collisionDetection={closestCenter}
            autoScroll={false}
            onDragStart={({ active: dragged }) => {
              if (active && !busy) setActiveDragId(String(dragged.id));
            }}
            onDragCancel={() => setActiveDragId(null)}
            onDragEnd={handleDragEnd}
          >
            <SortableContext
              items={channels.map(channelKey)}
              strategy={verticalListSortingStrategy}
            >
              <div className="voice-channels" role="list" aria-label={t('voice.channels')}>
                {Array.from({ length: 3 }, (_, index) => (
                  <SortableCard
                    key={channels[index] ? channelKey(channels[index]) : 'empty:' + index}
                    id={channels[index] ? channelKey(channels[index]) : 'empty:' + index}
                    dragLabel={t('model.dragSort')}
                    disabled={!active || busy || !channels[index] || channels.length < 2}
                  >
                    {(handle) => renderChannel(index, handle)}
                  </SortableCard>
                ))}
              </div>
            </SortableContext>
            {createPortal(
              <DragOverlay
                dropAnimation={null}
                zIndex={1002}
                className="pointer-events-none font-sans text-cyber-text"
              >
                {dragIndex >= 0 ? renderChannel(dragIndex, undefined, true) : null}
              </DragOverlay>,
              document.body
            )}
          </DndContext>
        </div>
        <section className="voice-card voice-microphone-card rounded-card">
          <div className="voice-section-heading">
            <h3>
              <Mic size={18} />
              {t('voice.microphone')}
            </h3>
          </div>
          <div className="voice-mic-controls">
            <MiniSelect
              className="voice-device-select"
              disabled={busy}
              value={config.microphone ?? ''}
              onChange={(value) => update('microphone', value || null)}
              options={[
                { id: '', label: t('voice.defaultMicrophone') },
                ...(config.microphone && !devices.includes(config.microphone)
                  ? [{ id: config.microphone, label: config.microphone }]
                  : []),
                ...devices.map((device) => ({ id: device, label: device })),
              ]}
            />
            <button
              type="button"
              className="voice-text-button"
              disabled={
                saving || pending || (voiceBusy(snapshot.phase) && snapshot.phase !== 'testing')
              }
              onClick={() => void (snapshot.phase === 'testing' ? state.stop() : state.start())}
            >
              <AudioLines size={15} />
              <span className="voice-mic-test-label">
                <span aria-hidden={snapshot.phase === 'testing'}>{t('voice.testMicrophone')}</span>
                <span aria-hidden={snapshot.phase !== 'testing'}>{t('voice.stopTest')}</span>
              </span>
            </button>
          </div>
          <div
            className="voice-meter"
            role="meter"
            aria-label={t('voice.microphoneLevel')}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(Math.max(0, snapshot.level) * 100)}
          >
            {Array.from({ length: 30 }, (_, index) => (
              <i key={index} className={snapshot.level * 30 > index ? 'lit' : ''} />
            ))}
          </div>
        </section>
        <section className="voice-card voice-shortcut-card rounded-card">
          <div className="voice-section-heading">
            <h3>
              <Keyboard size={18} />
              {t('voice.shortcut')}
            </h3>
          </div>
          <div className="voice-shortcut-controls">
            <fieldset
              aria-label={t('voice.shortcutMode')}
              disabled={busy}
              className="m-0 min-w-0 shrink-0 border-0 p-0 disabled:opacity-40"
            >
              <ViewModeTabs
                value={config.hotkeyMode ?? 'hold'}
                options={[
                  { value: 'hold', label: t('voice.shortcutHold') },
                  { value: 'toggle', label: t('voice.shortcutToggle') },
                ]}
                onChange={(value) => update('hotkeyMode', value)}
              />
            </fieldset>
            <div className="voice-shortcut-control">
              <ShortcutRecorder
                key={String(active)}
                active={active}
                enabled={config.hotkeyEnabled}
                binding={config.shortcut}
                disabled={busy}
                onSave={(shortcut) => save({ shortcut, hotkeyEnabled: true })}
              />
              {config.hotkeyEnabled && (
                <button
                  type="button"
                  className="voice-shortcut-clear"
                  aria-label={t('voice.disableShortcut')}
                  disabled={busy}
                  onClick={() => update('hotkeyEnabled', false)}
                >
                  <X size={16} />
                </button>
              )}
            </div>
          </div>
          <p className="voice-shortcut-description">{t('voice.shortcutCaution')}</p>
          {snapshot.shortcutDescription && (
            <span className="voice-status">{snapshot.shortcutDescription}</span>
          )}
        </section>
        <section className="voice-card voice-selection-settings rounded-card">
          <div className="voice-row">
            <span className="voice-selection-label">
              <Languages size={17} />
              <span>
                {t('voice.selection')}
                <span className="voice-shortcut-hint">
                  {t(selectionModel ? 'voice.selectionHint' : 'voice.selectionRequiresModel')}
                </span>
              </span>
            </span>
            <div className="voice-selection-controls">
              <fieldset disabled={busy || !active} className="voice-selection-model">
                <RemoteModelSelector
                  key={`${active}:${voiceActive}`}
                  models={textModels.map((model) => ({
                    id: model.internalId,
                    name: model.name,
                    icon: getModelIcon('', model.modelId),
                    iconKind: isSmartRouterId(model.internalId) ? ('router' as const) : undefined,
                  }))}
                  currentModelId={selectionModel ?? ''}
                  loading={false}
                  onSelect={(id) => {
                    if (active && !busy && id) update('selectionModel', id);
                  }}
                  placeholder={t('voice.selectModel')}
                />
              </fieldset>
              <MiniSelect
                value={config.selectionTranslationLanguage ?? 'zh-Hans'}
                disabled={busy || !selectionModel}
                dropUp
                options={selectionLanguages.map((language) => ({
                  id: language,
                  label: t(`voice.translation.${language}` as TKey),
                }))}
                onChange={(value) =>
                  update(
                    'selectionTranslationLanguage',
                    value as VoiceConfig['selectionTranslationLanguage']
                  )
                }
              />
              <fieldset
                aria-label={t('voice.selection')}
                disabled={busy || !selectionModel}
                className="m-0 min-w-0 shrink-0 border-0 p-0 disabled:opacity-40"
              >
                <ViewModeTabs
                  value={config.selectionTranslationEnabled && selectionModel ? 'on' : 'off'}
                  options={[
                    { value: 'on', label: t('voice.selectionOn') },
                    { value: 'off', label: t('voice.selectionOff') },
                  ]}
                  onChange={(value) => {
                    if (selectionModel) update('selectionTranslationEnabled', value === 'on');
                  }}
                />
              </fieldset>
            </div>
          </div>
        </section>
        <section className="voice-scale-card">
          <div className="voice-row voice-scale-row">
            <IslandScaleSlider
              scales={voiceIslandScales}
              value={scaleDraft ?? config.islandScale ?? 100}
              label={t('voice.islandScale')}
              disabled={voiceActive}
              onChange={setScaleDraft}
              onCommit={() => {
                if (scaleDraft === null) return;
                setScaleDraft(null);
                if (scaleDraft !== (config.islandScale ?? 100)) update('islandScale', scaleDraft);
              }}
            />
          </div>
        </section>
      </div>
    </div>
  );
}

function VoiceRoutingLines({
  stageRef,
  enabled,
  sourceOrder,
  layoutKey,
  channelCount,
  activeChannel,
}: {
  stageRef: RefObject<HTMLDivElement>;
  enabled: boolean;
  sourceOrder: string;
  layoutKey: string;
  channelCount: number;
  activeChannel: number | null;
}) {
  const [lines, setLines] = useState({ width: 0, height: 0, curves: [] as string[], arrow: '' });
  useEffect(() => {
    const stage = stageRef.current;
    const previewArea = stage?.querySelector('.voice-island-preview');
    const island = stage?.querySelector('.voice-island');
    const cards = stage?.querySelectorAll('.voice-channel');
    if (!stage || !previewArea || !island || cards?.length !== 3) return;
    const measure = () => {
      const box = stage.getBoundingClientRect();
      const preview = island.getBoundingClientRect();
      const nodes = Array.from(cards, (card) => card.getBoundingClientRect());
      const left = preview.right - box.left + 10;
      const right = nodes[0].left - box.left - 10;
      const y = preview.top + preview.height / 2 - box.top;
      const end = left + 8;
      const bend = (right + end) / 2;
      const visible = right - left > 40;
      setLines({
        width: box.width,
        height: box.height,
        curves: visible
          ? nodes.map((node) => {
              const center = node.top + node.height / 2 - box.top;
              return `M ${right} ${center} C ${bend} ${center} ${bend} ${y} ${end} ${y}`;
            })
          : [],
        arrow: visible ? `M ${left} ${y} L ${left + 10} ${y - 5} V ${y + 5} Z` : '',
      });
    };
    const frame = requestAnimationFrame(measure);
    const observer = new ResizeObserver(measure);
    [stage, previewArea, ...cards].forEach((element) => observer.observe(element));
    window.addEventListener('resize', measure);
    stage.addEventListener('transitionend', measure);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      window.removeEventListener('resize', measure);
      stage.removeEventListener('transitionend', measure);
    };
  }, [stageRef, sourceOrder, layoutKey]);
  return (
    <svg
      className={`voice-routing-lines${enabled && channelCount > 0 ? '' : ' is-disabled'}`}
      width="100%"
      height="100%"
      viewBox={`0 0 ${lines.width || 1} ${lines.height || 1}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      {lines.curves.map((d, index) => (
        <g key={index} className={index < channelCount ? undefined : 'is-disabled'}>
          <path className="free-model-route-path" d={d} />
          {enabled && index < channelCount && activeChannel === index && (
            <path
              className="free-model-route-glow"
              d={d}
              pathLength="1"
              ref={(path) => {
                if (path)
                  path.style.setProperty(
                    '--free-model-route-glow-span',
                    String(Math.min(1, Math.max(0.07, 20 / path.getTotalLength())))
                  );
              }}
            />
          )}
        </g>
      ))}
      <path className="free-model-route-arrow" d={lines.arrow} />
    </svg>
  );
}

export function VoiceInputPanel() {
  const { t } = useI18n();
  const [tab, setTab] = useState<'account' | 'model'>('account');
  const [directory, setDirectory] = useState<VoiceDirectoryEntry[]>(
    modelDirectory.speechProviders as VoiceDirectoryEntry[]
  );
  const state = useVoiceInputStore();
  const accounts = useVoiceAccounts();
  const models = useModelNexus();
  const active = useNavigationStore((navigation) => navigation.activePage === 'voice');
  const unavailable = !state.snapshot || state.pending || voiceBusy(state.snapshot?.phase);
  const disabled = unavailable || state.saving;
  useEffect(() => {
    if (!active) return;
    let disposed = false;
    let revision = 0;
    const load = () => {
      const attempt = ++revision;
      void getVoiceModels()
        .then((rows) => {
          if (!disposed && attempt === revision) useVoiceInputStore.setState({ models: rows });
        })
        .catch(() => {});
    };
    load();
    void modelApi
      .getModelDirectory()
      .then((remote) => {
        if (
          !disposed &&
          remote?.speechProviders?.length &&
          remote.speechProviders.every(
            (entry) =>
              Object.prototype.hasOwnProperty.call(voiceProtocols, entry.protocol) &&
              typeof entry.name === 'string' &&
              typeof entry.url === 'string' &&
              typeof entry.modelId === 'string' &&
              Array.isArray(entry.modelIds) &&
              entry.modelIds.every((id) => typeof id === 'string') &&
              typeof entry.baseUrl === 'string'
          )
        )
          setDirectory(remote.speechProviders);
      })
      .catch(() => {});
    window.addEventListener('models-changed', load);
    return () => {
      disposed = true;
      window.removeEventListener('models-changed', load);
    };
  }, [active]);
  const openModel = (entry?: VoiceDirectoryEntry) => {
    if (disabled) return;
    models.setModelModalDestination('voiceInput');
    models.setNewModelForm({
      name: entry?.name ?? '',
      baseUrl: entry?.baseUrl ?? '',
      anthropicUrl: '',
      apiKey: '',
      modelId: entry?.modelId ?? '',
      modelIdOptions: entry?.modelIds,
      voiceOptions: { protocol: entry?.protocol ?? 'openai' },
    });
    models.setEditingModelId(null);
    models.setKeyDestroyed(false);
    models.setShowApiKey(false);
    models.setShowAddModelModal(true);
  };
  return (
    <div
      className="voice-input-panel flex h-full flex-col overflow-hidden"
      data-saving={state.saving && !unavailable}
    >
      <div className="h-10 shrink-0 px-2 mb-2 flex items-center justify-between gap-2">
        <div className="min-w-0 flex-1">
          <ViewModeTabs
            fullWidth
            value={tab}
            onChange={setTab}
            options={[
              { value: 'account', label: t('voice.accountInput') },
              { value: 'model', label: t('btn.addModel') },
            ]}
          />
        </div>
      </div>
      <div className="flex-1 px-2 pt-0.5 pb-2 overflow-y-auto space-y-2">
        {tab === 'account' ? (
          <AccountProxyPanel
            voice
            disabled={disabled}
            accounts={accounts.accounts}
            accountCounts={accounts.accountCounts}
          />
        ) : (
          <>
            {state.models
              .filter(
                (model) =>
                  !state.snapshot?.config.channels.some(
                    (channel) => channel.kind === 'model' && channel.id === model.internalId
                  )
              )
              .map((model) => (
                <ModelListCard
                  compact
                  key={model.internalId}
                  model={model}
                  t={t}
                  selectionDisabled={disabled}
                  selection={<Plus size={22} className="text-cyber-text-muted" />}
                  onSelect={() => {
                    if (!disabled && state.snapshot)
                      void state.save({
                        channels: [
                          ...state.snapshot.config.channels,
                          { kind: 'model', id: model.internalId },
                        ],
                      });
                  }}
                />
              ))}
            {directory.map((entry) => (
              <DirectoryRow
                key={entry.name}
                name={entry.name}
                url={entry.url}
                iconSrc={getModelIcon(entry.name, '')}
                secondary={entry.modelId}
                onOpen={() =>
                  void shellOpen(entry.url).catch(() => window.open(entry.url, '_blank'))
                }
                openLabel={`Open ${entry.name} website`}
                add={{
                  onClick: () => openModel(entry),
                  label: `${t('btn.addModel')}: ${entry.name}`,
                  disabled,
                }}
              />
            ))}
          </>
        )}
      </div>
    </div>
  );
}
