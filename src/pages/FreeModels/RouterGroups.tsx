import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MutableRefObject,
  type ReactNode,
  type RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import { Lock, Plus, SquarePen, Trash2, Unlock, X } from 'lucide-react';
import * as api from '../../api/tauri';
import { RoutingToggle } from '../../components/RoutingToggle';
import { type ViewModeTabPosition } from '../../components/ViewModeTabGroup';
import { useConfirm } from '../../components/ConfirmDialog';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import { copyText } from '../../utils/copyText';
import { useFreeModels } from './provider';

const MAX_ROUTER_GROUPS = 7;

function RouterSettingsModal({
  initial,
  onClose,
  onSave,
}: {
  initial: api.SmartRouterSettings;
  onClose: () => void;
  onSave: (settings: api.SmartRouterSettings) => Promise<void>;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [form, setForm] = useState(initial);
  const [port, setPort] = useState(initial.port);
  const [saving, setSaving] = useState(false);
  const [pending, setPending] = useState(false);
  const request = useRef(0);
  const [error, setError] = useState('');
  const locked = form.apiKey.startsWith('enc:v1:');
  const keyMissing = !form.apiKey.trim();
  const valid =
    form.name.trim() && form.modelId.trim() && form.apiKey.trim() && port > 0 && port <= 65535;
  const busy = saving || pending;
  useEffect(
    () => () => {
      request.current++;
    },
    []
  );
  const errorMessage = (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    if (message.includes('SMART_ROUTER_PORT_IN_USE')) return t('freeModels.router.portInUse');
    if (message.includes('SMART_ROUTER_PORT_UNAVAILABLE'))
      return t('freeModels.router.portUnavailable');
    return message;
  };
  const updateField = async (field: 'port' | 'apiKey', read: () => Promise<string | null>) => {
    if (busy) return;
    const attempt = ++request.current;
    setPending(true);
    setError('');
    try {
      const value = await read();
      if (attempt === request.current && value !== null) {
        if (field === 'port') setPort(Number(value));
        else setForm((current) => ({ ...current, [field]: value }));
      }
    } catch (error) {
      if (attempt === request.current) setError(errorMessage(error));
    } finally {
      if (attempt === request.current) setPending(false);
    }
  };
  const randomizePort = () =>
    updateField('port', async () => String(await api.getSmartRouterAvailablePort(port)));
  const copy = async (value: string, encrypted = false) => {
    if (busy || !value.trim()) return;
    const attempt = ++request.current;
    try {
      const text = encrypted ? await api.decryptSecret(value) : value;
      if (attempt !== request.current) return;
      if (!text.trim()) throw new Error('Empty copied value');
      const ok = await copyText(text);
      if (attempt === request.current)
        showToast(
          ok ? 'success' : 'error',
          t(ok ? 'feedback.step1.copied' : 'feedback.step1.failed')
        );
    } catch {
      if (attempt === request.current) showToast('error', t('feedback.step1.failed'));
    }
  };
  const toggleKey = () => {
    if (!form.apiKey) return;
    return updateField('apiKey', async () => {
      const key = locked
        ? await api.decryptSecret(form.apiKey)
        : await api.encryptSecret(form.apiKey);
      if (!key || (!locked && !key.startsWith('enc:v1:')))
        throw new Error(t('error.requestFailed'));
      return key;
    });
  };
  const close = () => {
    if (!saving) onClose();
  };
  return createPortal(
    <div
      className="fixed inset-0 z-[9998] flex items-center justify-center"
      onKeyDown={(e) => {
        if (e.key === 'Escape') close();
      }}
    >
      <div className="absolute inset-0 bg-black/60" onClick={close} />
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="router-settings-title"
        aria-busy={busy || undefined}
        className="relative w-[450px] max-w-[90vw] overflow-hidden rounded-xl border border-cyber-border/30 bg-cyber-surface shadow-2xl"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!valid || busy) return;
          setSaving(true);
          setError('');
          const attempt = ++request.current;
          try {
            const apiKey = locked ? await api.decryptSecret(form.apiKey) : form.apiKey;
            if (!apiKey.trim()) throw new Error(t('error.requestFailed'));
            if (attempt !== request.current) return;
            await onSave({ ...form, port, apiKey });
            if (attempt === request.current) onClose();
          } catch (error) {
            if (attempt === request.current) setError(errorMessage(error));
          } finally {
            if (attempt === request.current) setSaving(false);
          }
        }}
      >
        <div className="h-px w-full bg-cyber-border" />
        <div className="flex items-center justify-between px-6 pb-4 pt-5">
          <div className="flex items-center gap-2">
            <span aria-hidden="true" className="font-mono text-sm text-cyber-text opacity-60">
              &gt;_
            </span>
            <span id="router-settings-title" className="text-base font-bold text-cyber-text">
              {t(initial.id ? 'freeModels.router.edit' : 'freeModels.router.add')}
            </span>
          </div>
          <button
            type="button"
            disabled={saving}
            onClick={close}
            aria-label={t('btn.close')}
            className="text-cyber-text-secondary transition-colors close-button"
          >
            <X size={18} />
          </button>
        </div>
        <fieldset disabled={busy} className="space-y-4 px-5 pb-5">
          <div>
            <label htmlFor="router-name" className="mb-1 block text-xs text-cyber-text-secondary">
              {t('model.name')}
            </label>
            <input
              id="router-name"
              type="text"
              autoFocus
              autoComplete="off"
              value={form.name}
              onChange={(e) => setForm((current) => ({ ...current, name: e.target.value }))}
              className="w-full rounded-button border border-cyber-border bg-cyber-input px-2 py-1.5 font-mono text-xs text-cyber-text focus:border-cyber-border focus:outline-none"
            />
          </div>
          {(['OpenAI', 'Anthropic'] as const).map((protocol) => (
            <div key={protocol}>
              <label
                htmlFor={`router-${protocol}`}
                className="mb-1 block text-xs text-cyber-text-secondary"
              >
                {t(protocol === 'OpenAI' ? 'model.openaiUrl' : 'model.anthropicUrl')}
              </label>
              <div className="relative">
                <input
                  id={`router-${protocol}`}
                  type="text"
                  readOnly
                  value={`127.0.0.1:${port}${protocol === 'OpenAI' ? '/v1' : ''}`}
                  className="w-full rounded-button border border-cyber-border bg-cyber-input py-1.5 pl-2 pr-20 font-mono text-xs text-cyber-text-secondary focus:border-cyber-border focus:outline-none"
                  style={{ cursor: 'default' }}
                />
                <div className="absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-2">
                  <button
                    type="button"
                    disabled={busy}
                    aria-label={`${t('model.copy')} ${protocol}`}
                    onClick={() =>
                      void copy(`http://127.0.0.1:${port}${protocol === 'OpenAI' ? '/v1' : ''}`)
                    }
                    className="text-xs text-cyber-text-secondary"
                  >
                    {t('model.copy')}
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    aria-label={`${t('freeModels.router.randomPort')} ${protocol}`}
                    onClick={() => void randomizePort()}
                    className="text-sm"
                  >
                    <span aria-hidden="true">🎲</span>
                  </button>
                </div>
              </div>
            </div>
          ))}
          <div>
            <label
              htmlFor="router-modelId"
              className="mb-1 block text-xs text-cyber-text-secondary"
            >
              {t('model.modelId')}
            </label>
            <div className="relative">
              <input
                id="router-modelId"
                type="text"
                autoComplete="off"
                aria-label={t('model.modelId')}
                value={form.modelId}
                onChange={(e) => setForm((current) => ({ ...current, modelId: e.target.value }))}
                className="w-full rounded-button border border-cyber-border bg-cyber-input py-1.5 pl-2 pr-16 font-mono text-xs text-cyber-text focus:border-cyber-border focus:outline-none"
              />
              <button
                type="button"
                disabled={busy || !form.modelId.trim()}
                aria-label={`${t('model.copy')} ${t('model.modelId')}`}
                onClick={() => void copy(form.modelId)}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-cyber-text-secondary"
              >
                {t('model.copy')}
              </button>
            </div>
          </div>
          <div>
            <label htmlFor="router-apiKey" className="mb-1 block text-xs text-cyber-text-secondary">
              {t('model.apiKey')}
            </label>
            <div className="relative">
              <input
                id="router-apiKey"
                type="text"
                autoComplete="off"
                readOnly={locked}
                aria-invalid={keyMissing || undefined}
                aria-describedby={keyMissing || locked ? 'router-key-hint' : undefined}
                value={locked ? '•••••••••••••••' : form.apiKey}
                onChange={(e) => setForm((current) => ({ ...current, apiKey: e.target.value }))}
                className="w-full rounded-button border border-cyber-border bg-cyber-input px-2 py-1.5 pr-20 font-mono text-xs text-cyber-text focus:border-cyber-border focus:outline-none"
              />
              <button
                type="button"
                disabled={busy || keyMissing}
                aria-label={`${t('model.copy')} ${t('model.apiKey')}`}
                onClick={() => void copy(form.apiKey, locked)}
                className="absolute right-8 top-1/2 -translate-y-1/2 text-xs text-cyber-text-secondary"
              >
                {t('model.copy')}
              </button>
              <button
                type="button"
                disabled={busy || !form.apiKey}
                aria-label={t(locked ? 'freeModels.router.unlockKey' : 'freeModels.router.lockKey')}
                onClick={() => void toggleKey()}
                className={`absolute right-2 top-1/2 -translate-y-1/2 transition-colors hover:opacity-80 ${!form.apiKey ? 'opacity-30' : ''}`}
              >
                {locked ? (
                  <Lock size={14} className="text-cyber-accent" />
                ) : (
                  <Unlock size={14} className="text-cyber-text/70" />
                )}
              </button>
            </div>
            <div
              id="router-key-hint"
              role={keyMissing ? 'alert' : undefined}
              aria-hidden={!keyMissing && !locked}
              className={`mt-1 text-xs leading-tight ${keyMissing ? 'text-red-400' : 'text-cyber-text/60'} ${keyMissing || locked ? '' : 'invisible'}`}
            >
              {t(keyMissing ? 'freeModels.router.apiKeyRequired' : 'key.encrypted')}
            </div>
          </div>
          {error && (
            <p role="alert" className="break-words text-xs text-red-400">
              {error}
            </p>
          )}
        </fieldset>
        <div className="flex border-t border-cyber-border">
          <button
            type="button"
            disabled={saving}
            onClick={close}
            className="flex-1 border-r border-cyber-border px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary transition-all enabled:hover:bg-cyber-elevated enabled:hover:text-cyber-text"
          >
            {t('model.escCancel')}
          </button>
          <button
            type="submit"
            disabled={!valid || busy}
            aria-busy={saving || undefined}
            className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text transition-all enabled:hover:bg-cyber-text/10 disabled:text-cyber-text-muted"
          >
            {t('model.enterSave')}
          </button>
        </div>
      </form>
    </div>,
    document.body
  );
}

export function RouterGroups({
  hubRef,
  restoreFocusRef,
  tabPositionRef: _tabPositionRef,
  refreshAction,
}: {
  hubRef: RefObject<HTMLDivElement>;
  restoreFocusRef: MutableRefObject<boolean>;
  tabPositionRef: MutableRefObject<ViewModeTabPosition | null>;
  refreshAction?: ReactNode;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();
  const {
    routers,
    activeRouterId,
    layout,
    groupPending,
    routerEnabled,
    selectRouter,
    saveRouter,
    deleteRouter,
    setGroupEnabled,
  } = useFreeModels();
  const [settings, setSettings] = useState<api.SmartRouterSettings | null>(null);
  const [reading, setReading] = useState(false);
  const request = useRef(0);
  const selectedTabRef = useRef<HTMLButtonElement>(null);
  const busy = groupPending || reading;
  const canAdd = routers.length < MAX_ROUTER_GROUPS;
  useLayoutEffect(() => {
    const tab = selectedTabRef.current;
    if (layout !== 'cards' || !tab || tab.offsetParent === null) {
      restoreFocusRef.current = false;
      return;
    }
    if (busy) return;
    if (restoreFocusRef.current) {
      restoreFocusRef.current = false;
      tab.focus({ preventScroll: true });
    }
  }, [activeRouterId, layout, routers.length, busy, restoreFocusRef]);
  const selectTab = (id: string) => {
    if (busy || id === activeRouterId) return;
    restoreFocusRef.current = true;
    void selectRouter(id);
  };
  const invalidateRequest = useCallback(() => {
    request.current++;
  }, []);
  useEffect(() => invalidateRequest, [invalidateRequest]);
  const edit = async (id: string) => {
    const attempt = ++request.current;
    setReading(true);
    try {
      const value = await api.getSmartRouterSettings(id);
      if (attempt === request.current) setSettings(value);
    } catch {
      if (attempt === request.current) showToast('error', t('error.requestFailed'));
    } finally {
      if (attempt === request.current) setReading(false);
    }
  };
  const add = async () => {
    if (busy || !canAdd) return;
    const attempt = ++request.current;
    setReading(true);
    try {
      const port = await api.getSmartRouterAvailablePort();
      if (attempt === request.current)
        setSettings({
          name: `${t('freeModels.router.title')} #${routers.length + 1}`,
          modelId: 'auto',
          port,
          apiKey: `eb-local-${crypto.randomUUID().replace(/-/g, '')}`,
        });
    } catch {
      if (attempt === request.current) showToast('error', t('freeModels.router.portUnavailable'));
    } finally {
      if (attempt === request.current) setReading(false);
    }
  };
  const remove = async (router: api.SmartRouterConfig) => {
    if (routers.length <= 1 || busy) return;
    const accepted = await confirm({
      title: t('freeModels.router.delete'),
      message: t('freeModels.router.deleteConfirm').replace('{name}', router.name),
      confirmText: t('btn.remove'),
      cancelText: t('btn.cancel'),
      type: 'danger',
    });
    if (accepted) {
      try {
        await deleteRouter(router.id);
      } catch {
        showToast('error', t('error.requestFailed'));
      }
    }
  };
  const groupNavigation = (
    <div className="free-model-router-group-toolbar overflow-hidden rounded-button border border-cyber-border">
      <div
        role="tablist"
        aria-label={t('freeModels.router.title')}
        className="free-model-router-tabs"
      >
        {routers.map((router, index) => (
          <button
            key={router.id}
            type="button"
            role="tab"
            ref={router.id === activeRouterId ? selectedTabRef : undefined}
            id={`router-group-tab-${router.id}`}
            aria-label={router.name}
            aria-selected={router.id === activeRouterId}
            aria-controls={`router-group-panel-${router.id}`}
            tabIndex={router.id === activeRouterId ? 0 : -1}
            disabled={busy}
            onClick={() => selectTab(router.id)}
            onKeyDown={(event) => {
              if (busy || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
              event.preventDefault();
              const next =
                event.key === 'Home'
                  ? 0
                  : event.key === 'End'
                    ? routers.length - 1
                    : (index + (event.key === 'ArrowRight' ? 1 : routers.length - 1)) %
                      routers.length;
              const tabs =
                event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>(
                  '[role="tab"]'
                );
              tabs?.[next]?.focus();
            }}
            className={`free-model-router-tab text-xs transition-colors ${router.id === activeRouterId ? 'bg-cyber-accent text-white font-semibold hover:bg-cyber-accent-secondary' : routerEnabled && router.groupEnabled ? 'font-bold text-cyber-accent hover:text-cyber-text' : 'text-cyber-text-secondary hover:text-cyber-text'}`}
          >
            {index + 1}
          </button>
        ))}
      </div>
      {canAdd && (
        <button
          type="button"
          disabled={busy}
          onClick={add}
          aria-label={t('freeModels.router.add')}
          className="free-model-router-tab text-cyber-text-secondary hover:bg-cyber-elevated hover:text-cyber-text"
        >
          <Plus size={16} />
        </button>
      )}
    </div>
  );
  const hub = (router: api.SmartRouterConfig) => {
    const list = layout === 'list';
    const toggle = (
      <div className={routerEnabled ? undefined : 'opacity-50'}>
        <RoutingToggle
          hideLabel
          label={router.name}
          checked={router.groupEnabled}
          disabled={!routerEnabled}
          pending={busy}
          onChange={(enabled) => void setGroupEnabled(router.id, enabled)}
        />
      </div>
    );
    const editButton = (
      <button
        type="button"
        aria-label={`${t('freeModels.router.edit')} ${router.name}`}
        disabled={busy}
        onClick={() => void edit(router.id)}
        className={`hover:text-cyber-text ${list ? 'flex h-7 items-center justify-center' : 'flex h-7 w-7 items-center justify-center text-cyber-text-secondary'}`}
      >
        <SquarePen size={15} />
      </button>
    );
    const deleteButton = (
      <button
        type="button"
        aria-label={`${t('freeModels.router.delete')} ${router.name}`}
        disabled={busy || routers.length <= 1}
        onClick={() => void remove(router)}
        className={`hover:text-red-400 ${routers.length <= 1 ? 'opacity-30' : ''} ${list ? 'flex h-7 items-center justify-center' : 'flex h-7 w-7 items-center justify-center text-cyber-text-secondary'}`}
      >
        <Trash2 size={15} />
      </button>
    );
    const details = (
      <>
        <div
          className={`free-model-router-heading flex min-w-0 items-center gap-2 ${list ? 'justify-center' : 'free-model-router-card-heading'}`}
        >
          <span
            className={`truncate font-semibold text-cyber-text ${list ? 'text-sm' : 'flex-1 text-lg leading-none'}`}
          >
            {router.name}
          </span>
          {!list && (
            <div className="flex shrink-0 items-center gap-1">
              {editButton}
              {deleteButton}
              <div className="ml-1.5">{toggle}</div>
            </div>
          )}
        </div>
        <div className={`free-model-router-endpoints gap-3 ${list ? 'grid grid-cols-2' : 'flex'}`}>
          {[
            ['OpenAI', router.baseUrl],
            ['Anthropic', router.baseUrl.replace(/\/v1\/?$/, '')],
          ].map(([protocol, url]) => (
            <button
              key={protocol}
              type="button"
              aria-label={`${t('btn.copy')} ${protocol}`}
              className={`min-w-0 rounded-button bg-cyber-bg/60 text-left ${list ? 'flex items-center gap-2 px-3 py-2' : 'free-model-router-card-endpoint'}`}
              onClick={(e) => {
                e.stopPropagation();
                void copyText(url).then((ok) =>
                  showToast(
                    ok ? 'success' : 'error',
                    t(ok ? 'feedback.step1.copied' : 'feedback.step1.failed')
                  )
                );
              }}
            >
              <span className="block text-[10px] font-semibold text-cyber-text-muted">
                {protocol}
              </span>
              <span className="block break-all font-mono text-xs text-cyber-text-secondary">
                {url.replace(/^https?:\/\//, '')}
              </span>
            </button>
          ))}
        </div>
      </>
    );
    return (
      <div
        key={router.id}
        ref={hubRef}
        data-router-id={list ? router.id : undefined}
        className={`free-model-router-hub relative z-10 rounded-xl ${list ? 'free-model-router-hub-bar' : 'free-model-router-group'}`}
      >
        {list ? (
          details
        ) : (
          <div
            role="tabpanel"
            id={`router-group-panel-${router.id}`}
            aria-labelledby={`router-group-tab-${router.id}`}
            data-router-id={router.id}
            className="free-model-router-card-details"
          >
            {details}
          </div>
        )}
        <div
          className={
            list
              ? 'free-model-list-actions text-cyber-text-muted'
              : 'free-model-router-card-actions flex items-center justify-between gap-3'
          }
          onClick={(e) => e.stopPropagation()}
        >
          {list ? (
            <>
              {deleteButton}
              {editButton}
              {toggle}
            </>
          ) : (
            <>{groupNavigation}</>
          )}
        </div>
      </div>
    );
  };
  return (
    <>
      {layout === 'cards' ? (
        <div className="free-model-router-switcher relative z-10" aria-busy={busy || undefined}>
          {routers.length > 1 &&
            ([-1, 1] as const).map((direction) => (
              <button
                key={direction}
                type="button"
                disabled={busy}
                aria-label={t(
                  direction < 0 ? 'freeModels.router.previous' : 'freeModels.router.next'
                )}
                onClick={() => {
                  const index = routers.findIndex((router) => router.id === activeRouterId);
                  const next = (index + direction + routers.length) % routers.length;
                  selectTab(routers[next].id);
                }}
                className={`free-model-router-page-arrow absolute top-1/2 -translate-y-1/2 flex h-20 w-16 items-center justify-center text-cyber-accent disabled:opacity-30 ${direction < 0 ? 'right-full mr-4' : 'left-full ml-4'}`}
              >
                <svg
                  aria-hidden="true"
                  width="48"
                  height="48"
                  viewBox="0 0 48 48"
                  fill="none"
                  className={`free-model-router-page-chevron ${direction < 0 ? 'is-previous' : ''}`}
                >
                  <path
                    d={direction < 0 ? 'M 31 8 L 15 24 L 31 40' : 'M 17 8 L 33 24 L 17 40'}
                    stroke="currentColor"
                    strokeWidth="8"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
              </button>
            ))}
          {routers.filter((router) => router.id === activeRouterId).map(hub)}
        </div>
      ) : (
        <>
          {routers.filter((router) => router.id === activeRouterId).map(hub)}
          <div className="relative z-10 flex min-h-[40px] items-center justify-end gap-2">
            <div
              className="flex min-w-0 items-center gap-1 overflow-x-auto"
              role="group"
              aria-label={t('freeModels.router.title')}
            >
              {routers.map((router, index) => (
                <button
                  key={router.id}
                  type="button"
                  disabled={busy}
                  aria-label={router.name}
                  aria-pressed={router.id === activeRouterId}
                  onClick={() => void selectRouter(router.id)}
                  className={`free-model-router-list-button w-7 shrink-0 rounded-button px-1 font-mono text-xs ${router.id === activeRouterId ? 'bg-cyber-accent/15 text-cyber-accent' : routerEnabled && router.groupEnabled ? 'font-bold text-cyber-accent' : 'text-cyber-text-muted'}`}
                >
                  {index + 1}
                </button>
              ))}
              {canAdd && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={add}
                  aria-label={t('freeModels.router.add')}
                  className="free-model-router-list-button shrink-0 rounded-button text-cyber-text-muted"
                >
                  <Plus size={14} />
                </button>
              )}
            </div>
            {refreshAction}
          </div>
        </>
      )}
      {settings && (
        <RouterSettingsModal
          key={settings.id ?? 'new'}
          initial={settings}
          onClose={() => setSettings(null)}
          onSave={saveRouter}
        />
      )}
    </>
  );
}
