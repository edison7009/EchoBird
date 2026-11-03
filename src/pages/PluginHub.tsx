import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import {
  Check,
  ChevronDown,
  ExternalLink,
  FolderOpen,
  GitBranch,
  Plus,
  Puzzle,
  RefreshCw,
  Search,
  ShieldCheck,
  X,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { gt } from 'semver';
import { useI18n } from '../hooks/useI18n';
import type { TKey } from '../i18n/types';
import { ViewModeTabs } from '../components/ViewModeTabs';
import { PageActionButton } from '../components/PageActionButton';
import { RoutingToggle } from '../components/RoutingToggle';
import { useConfirm } from '../components/ConfirmDialog';
import { useToast } from '../components/Toast';
import { usePluginsStore } from '../stores/pluginsStore';
import { useNavigationStore } from '../stores/navigationStore';
import { ModelCardSkeleton } from '../components/cards/ModelCard';
import { getDecodedImage, rememberDecodedImage } from '../utils/toolIcons';
import * as api from '../api/plugins';
import { openExternal } from '../api/tauri';
import type {
  PluginCandidate,
  PluginCategory,
  PluginManifest,
  PluginSource,
  PluginRegistry,
  PluginIndexRegion,
  PluginRegion,
} from '../api/plugins';
import './PluginHub.css';
import { PluginProductControls } from './PluginProductControls';

const categories = ['application', 'account', 'appearance', 'feature', 'other'] as const;
const accountNoticeKey = 'echobird-plugin-account-notice-dismissed';

const sourceLabel = {
  international: 'plugins.international',
  china: 'plugins.china',
  cnb: 'plugins.cnb',
} as const satisfies Record<PluginRegion, TKey>;
const sourceRepository: Record<PluginRegion, string> = {
  international: 'https://github.com/edison7009/echobird-plugin',
  china: 'https://gitcode.com/edison7009/echobird-plugin',
  cnb: 'https://cnb.cool/echobird/echobird-plugin',
};

const indexRegion = (region: PluginRegion): PluginIndexRegion =>
  region === 'cnb' ? 'china' : region;

function pluginDescription(
  manifest: PluginManifest,
  registry: PluginRegistry,
  region: PluginRegion
): string {
  return (
    registry.plugins.find((plugin) => plugin.id === `${manifest.publisher}.${manifest.name}`)
      ?.descriptions?.[indexRegion(region)] ?? manifest.description
  );
}

export function pluginCategories(manifest: PluginManifest): PluginCategory[] {
  return manifest.categories?.length ? manifest.categories : [pluginCategory(manifest)];
}

function pluginCategory(manifest: PluginManifest): PluginCategory {
  if (manifest.category)
    return categories.includes(manifest.category) ? manifest.category : 'other';
  if (manifest.echobird.contributes.providers?.length) return 'account';
  return manifest.echobird.contributes.pages.length ? 'application' : 'other';
}

function PluginIcon({ icon, clientIcon }: { icon?: string | null; clientIcon?: string }) {
  const local =
    clientIcon && /^[a-z0-9]+(?:-[a-z0-9]+)*\.(svg|png)$/.test(clientIcon)
      ? `/icons/tools/${clientIcon}`
      : undefined;
  return (
    <PluginIconImage
      key={`${local ?? ''}:${icon ?? ''}`}
      icon={local ?? icon}
      fallback={local ? icon : undefined}
    />
  );
}

function PluginIconImage({ icon, fallback }: { icon?: string | null; fallback?: string | null }) {
  const [loaded, setLoaded] = useState(() => !!icon && !!getDecodedImage(icon));
  const [failed, setFailed] = useState(false);
  const loading = !!icon && !loaded && !failed;
  if (failed && fallback && fallback !== icon) {
    return <PluginIconImage key={fallback} icon={fallback} />;
  }
  return (
    <span className="ph-plugin-icon" data-ready={loaded} aria-busy={loading || undefined}>
      {icon && !failed ? (
        <>
          {loading && <span className="ph-icon-skeleton" aria-hidden="true" />}
          <img
            src={icon}
            alt=""
            decoding="async"
            referrerPolicy="no-referrer"
            onLoad={(event) => {
              const image = event.currentTarget;
              void image.decode().then(
                () => {
                  rememberDecodedImage(icon, image);
                  setLoaded(true);
                },
                () => setFailed(true)
              );
            }}
            onError={() => setFailed(true)}
          />
        </>
      ) : (
        <Puzzle size={24} aria-hidden="true" />
      )}
    </span>
  );
}

const permissionLabels: Record<string, TKey> = {
  'model.request': 'plugins.modelPermission',
  'account.code': 'plugins.accountCodePermission',
  'clipboard.write': 'plugins.clipboardPermission',
  'extension.background': 'plugins.backgroundPermission',
  configuration: 'plugins.configurationPermission',
  storage: 'plugins.storagePermission',
  commands: 'plugins.commandsPermission',
  events: 'plugins.eventsPermission',
  'text.process': 'plugins.textPermission',
};

function Permissions({ manifest }: { manifest: PluginManifest }) {
  const { t } = useI18n();
  return (
    <ul className="ph-permissions">
      {(manifest.echobird.permissions.length ? manifest.echobird.permissions : ['none']).map(
        (permission) => (
          <li key={permission}>
            <ShieldCheck size={15} aria-hidden="true" />
            {t(permissionLabels[permission] ?? 'plugins.noPermissions')}
          </li>
        )
      )}
    </ul>
  );
}

function PluginImportDialog({
  kind,
  value = '',
  onClose,
}: {
  kind: PluginSource;
  value?: string;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const region = usePluginsStore((state) => state.region);
  const install = usePluginsStore((state) => state.install);
  const [address, setAddress] = useState(value);
  const [candidate, setCandidate] = useState<PluginCandidate | null>(null);
  const [pending, setPending] = useState(kind === 'catalog');
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const lifecycle = useRef({
    active: true,
    revision: 0,
    pending: false,
    candidate: null as PluginCandidate | null,
  });

  const preview = useCallback(
    async (source: string) => {
      const current = lifecycle.current;
      if (current.pending || !source.trim()) return;
      const revision = current.revision;
      current.pending = true;
      setPending(true);
      setError(null);
      try {
        const result = await api.previewPlugin(kind, source.trim(), region);
        if (!current.active || current.revision !== revision) {
          await api.cancelPlugin(result.token);
          return;
        }
        current.candidate = result;
        setCandidate(result);
      } catch (error) {
        if (current.active && current.revision === revision) setError(String(error));
      } finally {
        if (current.revision === revision) {
          current.pending = false;
          if (current.active) setPending(false);
        }
      }
    },
    [kind, region]
  );

  useEffect(() => {
    const current = lifecycle.current;
    current.active = true;
    dialog.current?.showModal();
    if (kind === 'catalog') {
      const revision = current.revision;
      current.pending = true;
      void api
        .previewPlugin(kind, value, region)
        .then(async (result) => {
          if (!current.active || revision !== current.revision) {
            await api.cancelPlugin(result.token);
            return;
          }
          current.candidate = result;
          setCandidate(result);
        })
        .catch((error) => {
          if (current.active && revision === current.revision) setError(String(error));
        })
        .finally(() => {
          if (revision === current.revision) {
            current.pending = false;
            if (current.active) setPending(false);
          }
        });
    }
    return () => {
      current.active = false;
      current.revision++;
      current.pending = false;
      if (current.candidate) void api.cancelPlugin(current.candidate.token).catch(() => {});
    };
  }, [kind, region, value]);

  const commit = async () => {
    if (!candidate || lifecycle.current.pending) return;
    lifecycle.current.pending = true;
    setInstalling(true);
    setError(null);
    try {
      await install(candidate.token);
      if (lifecycle.current.active) onClose();
    } catch (error) {
      if (lifecycle.current.active) setError(String(error));
    } finally {
      lifecycle.current.pending = false;
      if (lifecycle.current.active) setInstalling(false);
    }
  };

  return (
    <dialog
      className="ph-modal"
      ref={dialog}
      aria-labelledby="plugin-import-title"
      onCancel={(event) => {
        event.preventDefault();
        if (!installing) onClose();
      }}
    >
      <div className="ph-modal-heading">
        <h2 id="plugin-import-title">
          {t(
            candidate || kind === 'catalog'
              ? 'plugins.install'
              : kind === 'directory'
                ? 'plugins.directory'
                : 'plugins.repository'
          )}
        </h2>
        <button
          className="close-button"
          disabled={installing}
          aria-label={t('plugins.close')}
          onClick={onClose}
        >
          <X size={18} />
        </button>
      </div>
      {candidate ? (
        <>
          <h3>{candidate.manifest.displayName}</h3>
          <p>{candidate.manifest.description}</p>
          <p className="ph-muted">
            {candidate.manifest.publisher} · {candidate.manifest.version}
          </p>
          <p className="ph-muted">{candidate.source}</p>
          <h3>{t('plugins.permissions')}</h3>
          <Permissions manifest={candidate.manifest} />
        </>
      ) : kind === 'catalog' ? (
        <p>{t('plugins.checking')}</p>
      ) : (
        <>
          <label className="ph-field">
            {t(kind === 'directory' ? 'plugins.directory' : 'plugins.repository')}
            <input
              autoFocus
              value={address}
              disabled={pending}
              onChange={(event) => setAddress(event.target.value)}
              placeholder={kind === 'directory' ? '' : t('plugins.repositoryPlaceholder')}
              onKeyDown={(event) => {
                if (event.key === 'Enter') void preview(address);
              }}
            />
          </label>
          {kind === 'directory' && (
            <button
              className="ph-secondary"
              disabled={pending}
              onClick={async () => {
                try {
                  const selected = await open({ directory: true, multiple: false });
                  if (lifecycle.current.active && typeof selected === 'string')
                    setAddress(selected);
                } catch (error) {
                  if (lifecycle.current.active) setError(String(error));
                }
              }}
            >
              <FolderOpen size={16} />
              {t('plugins.chooseFolder')}
            </button>
          )}
        </>
      )}
      {error && (
        <p role="alert" className="ph-error">
          {error}
        </p>
      )}
      <div className="ph-modal-footer">
        <button className="ph-secondary" disabled={installing} onClick={onClose}>
          {t('plugins.cancel')}
        </button>
        <button
          className="ph-primary"
          disabled={pending || installing || (!candidate && !address.trim())}
          onClick={() => void (candidate ? commit() : preview(address))}
        >
          {t(pending ? 'plugins.checking' : candidate ? 'plugins.install' : 'plugins.continue')}
        </button>
      </div>
    </dialog>
  );
}

function moveMenuFocus(event: ReactKeyboardEvent<HTMLDivElement>) {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  const items = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]')
  );
  if (!items.length) return;
  event.preventDefault();
  const current = items.indexOf(document.activeElement as HTMLButtonElement);
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? items.length - 1
        : (current + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length;
  items[next].focus();
}

export function PluginHubMain({ active }: { active: boolean }) {
  const { t, locale } = useI18n();
  const { showToast } = useToast();
  const store = usePluginsStore();
  const loadPlugins = usePluginsStore((state) => state.load);
  const syncRegion = usePluginsStore((state) => state.syncRegion);
  useLayoutEffect(() => syncRegion(locale), [locale, syncRegion]);
  const accountPluginsRequest = useNavigationStore((state) => state.accountPluginsRequest);
  const consumeAccountPluginsRequest = useNavigationStore(
    (state) => state.consumeAccountPluginsRequest
  );
  const [tab, setTab] = useState<'discover' | 'installed'>('discover');
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState<PluginCategory | 'all'>('all');
  const [accountNoticeDismissed, setAccountNoticeDismissed] = useState(() => {
    try {
      return localStorage.getItem(accountNoticeKey) === 'true';
    } catch {
      return false;
    }
  });
  const categoryTabs = useRef<HTMLDivElement>(null);
  const categoryUnderline = useRef<HTMLSpanElement>(null);
  const [menu, setMenu] = useState<'source' | 'add' | null>(null);
  const toolbar = useRef<HTMLDivElement>(null);
  const [source, setSource] = useState<{ kind: PluginSource; value?: string } | null>(null);
  const [catalogCandidate, setCatalogCandidate] = useState<string | null>(null);
  useEffect(() => {
    if (!active) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- Leaving cancels the import dialog.
      setSource(null);
      setCatalogCandidate(null);
      setMenu(null);
    }
  }, [active]);

  useEffect(() => {
    if (
      !active ||
      accountPluginsRequest == null ||
      useNavigationStore.getState().accountPluginsRequest !== accountPluginsRequest
    )
      return;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- An account entry opens the complete marketplace.
    setTab('discover');
    setCategory('all');
    setQuery('');
    store.select(null);
    consumeAccountPluginsRequest(accountPluginsRequest);
    showToast('info', t('plugins.loginRequiresPlugin'));
  }, [active, consumeAccountPluginsRequest, accountPluginsRequest, showToast, store, t]);

  useEffect(() => {
    if (active) void loadPlugins();
  }, [active, loadPlugins]);

  useEffect(() => {
    if (!menu) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Element;
      if (!toolbar.current?.contains(target) || !target.closest('.ph-menu-anchor')) setMenu(null);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      toolbar.current?.querySelector<HTMLButtonElement>('[aria-expanded="true"]')?.focus();
      setMenu(null);
    };
    toolbar.current
      ?.querySelector<HTMLButtonElement>(
        '[role="menuitemradio"][aria-checked="true"], [role="menuitem"]'
      )
      ?.focus();
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', outside);
      document.removeEventListener('keydown', escape);
    };
  }, [menu]);

  const entries =
    tab === 'installed'
      ? store.installed.map((plugin) => ({ id: plugin.id, manifest: plugin.manifest }))
      : store.registry.plugins.flatMap((plugin) =>
          plugin.releases[0] ? [{ id: plugin.id, manifest: plugin.releases[0].manifest }] : []
        );
  useLayoutEffect(() => {
    const tabs = categoryTabs.current;
    const underline = categoryUnderline.current;
    if (!tabs || !underline) return;
    let frame: number | undefined;
    const update = () => {
      const selected = tabs.querySelector<HTMLButtonElement>('[aria-pressed="true"]');
      if (!selected?.offsetWidth) return;
      if (!underline.style.width) {
        underline.style.transition = 'none';
        frame = requestAnimationFrame(() => underline.style.removeProperty('transition'));
      }
      underline.style.width = `${selected.offsetWidth}px`;
      underline.style.transform = `translate(${selected.offsetLeft}px, ${selected.offsetTop + selected.offsetHeight - 2}px)`;
      underline.style.visibility = 'visible';
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(tabs);
    tabs.querySelectorAll('button').forEach((button) => observer.observe(button));
    return () => {
      observer.disconnect();
      if (frame !== undefined) {
        cancelAnimationFrame(frame);
        underline.style.removeProperty('transition');
      }
    };
  }, [category, t]);
  const filtered = entries.filter(({ id, manifest }) => {
    const providerTargets = manifest.echobird.contributes.providers ?? [];
    const matchesText =
      `${id} ${manifest.displayName} ${manifest.description} ${pluginDescription(manifest, store.registry, store.region)} ${providerTargets
        ?.flatMap((provider) => [
          provider.id,
          ...provider.applications,
          provider.pageId,
          ...(provider.searchTerms ?? []),
        ])
        .join(' ')}`
        .toLowerCase()
        .includes(query.toLowerCase());
    return (category === 'all' || pluginCategories(manifest).includes(category)) && matchesText;
  });
  const skeleton =
    store.loading &&
    !store.error &&
    (tab === 'installed'
      ? !store.loaded && store.installed.length === 0
      : store.registry.plugins.length === 0);
  return (
    <div className="ph-main">
      <div className="ph-toolbar" ref={toolbar}>
        <ViewModeTabs
          value={tab}
          onChange={(value) => {
            setTab(value);
            setMenu(null);
          }}
          options={[
            { value: 'discover', label: t('plugins.discover') },
            { value: 'installed', label: t('plugins.installed') },
          ]}
        />
        {tab === 'discover' && (
          <div className="ph-menu-anchor ph-region">
            <PageActionButton
              aria-label={t('plugins.sources')}
              aria-haspopup="menu"
              aria-expanded={menu === 'source'}
              disabled={store.loading || store.busy}
              onClick={() => setMenu(menu === 'source' ? null : 'source')}
            >
              {t(sourceLabel[store.region])}
              <ChevronDown size={13} />
            </PageActionButton>
            {menu === 'source' && (
              <div
                className="ph-menu ph-region-menu"
                role="menu"
                aria-label={t('plugins.sources')}
                onKeyDown={moveMenuFocus}
              >
                {(['international', 'china', 'cnb'] as const).map((region) => (
                  <button
                    key={region}
                    role="menuitemradio"
                    aria-checked={store.region === region}
                    onClick={() => {
                      store.setRegion(region);
                      setMenu(null);
                    }}
                  >
                    {t(sourceLabel[region])}
                    {store.region === region && <Check size={14} className="ph-menu-check" />}
                  </button>
                ))}
              </div>
            )}
            <button
              type="button"
              className="ph-source-link"
              aria-label={`${t('plugins.source')}: ${t(sourceLabel[store.region])}`}
              onClick={() => {
                setMenu(null);
                void openExternal(sourceRepository[store.region]).catch((error) =>
                  showToast('error', String(error))
                );
              }}
            >
              <ExternalLink size={18} aria-hidden="true" />
            </button>
          </div>
        )}
        <div className="ph-search">
          <Search size={16} />
          <input
            aria-label={t('plugins.search')}
            placeholder={t('plugins.search')}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
            }}
          />
          {query && (
            <button
              aria-label={t('plugins.clear')}
              onClick={() => {
                setQuery('');
              }}
            >
              <X size={15} />
            </button>
          )}
        </div>
        <div className="ph-actions">
          <PageActionButton
            icon={RefreshCw}
            aria-label={t('plugins.refresh')}
            spinning={store.loading}
            disabled={store.loading || store.busy}
            onClick={() => void store.refresh()}
          />
          <div className="ph-menu-anchor">
            <PageActionButton
              icon={Plus}
              aria-haspopup="menu"
              aria-expanded={menu === 'add'}
              onClick={() => setMenu(menu === 'add' ? null : 'add')}
            >
              {t('plugins.add')}
              <ChevronDown size={13} />
            </PageActionButton>
            {menu === 'add' && (
              <div className="ph-menu" role="menu" onKeyDown={moveMenuFocus}>
                <button
                  role="menuitem"
                  onClick={() => {
                    setSource({ kind: 'repository' });
                    setMenu(null);
                  }}
                >
                  <GitBranch size={16} />
                  {t('plugins.repository')}
                </button>
                <button
                  role="menuitem"
                  onClick={() => {
                    setSource({ kind: 'directory' });
                    setMenu(null);
                  }}
                >
                  <FolderOpen size={16} />
                  {t('plugins.directory')}
                </button>
              </div>
            )}
          </div>
        </div>
      </div>
      <div ref={categoryTabs} className="ph-categories relative mb-5 flex shrink-0 flex-wrap gap-1">
        {(['all', ...categories] as const).map((value) => (
          <button
            key={value}
            onClick={() => setCategory(value)}
            aria-pressed={category === value}
            className={`border-b-2 border-transparent px-3 py-1.5 text-[14px] font-bold transition-colors outline-none ${
              category === value
                ? 'text-cyber-text'
                : 'text-cyber-text-secondary hover:text-cyber-text'
            }`}
          >
            {t(value === 'all' ? 'toolCat.all' : `plugins.category.${value}`)}
          </button>
        ))}
        <span
          ref={categoryUnderline}
          aria-hidden="true"
          className="pointer-events-none absolute left-0 top-0 h-0.5 bg-cyber-border transition-[transform,width] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:transition-none"
          style={{ visibility: 'hidden' }}
        />
      </div>
      {!accountNoticeDismissed && (
        <div className="ph-account-notice">
          <p>{t('plugins.accountNotice')}</p>
          <button
            onClick={() => {
              setAccountNoticeDismissed(true);
              try {
                localStorage.setItem(accountNoticeKey, 'true');
              } catch {
                // Dismiss for this visit when local preferences are unavailable.
              }
            }}
          >
            {t('plugins.accountNoticeDismiss')}
          </button>
        </div>
      )}
      {store.error && (
        <p role="alert" className="ph-error">
          {store.error}
        </p>
      )}
      <div className="ph-results">
        <div className="ph-grid" aria-busy={skeleton || undefined}>
          {filtered.map(({ id, manifest }) => {
            const installed = store.installed.find((plugin) => plugin.id === id);
            return (
              <article key={id} className={`ph-card ${store.selected === id ? 'selected' : ''}`}>
                <button
                  className="ph-card-select"
                  aria-pressed={store.selected === id}
                  onClick={() => store.select(id)}
                >
                  <PluginIcon
                    clientIcon={
                      store.registry.plugins.find((plugin) => plugin.id === id)?.clientIcon
                    }
                    icon={
                      installed?.icon ??
                      store.registry.plugins.find((plugin) => plugin.id === id)?.icons?.[
                        indexRegion(store.region)
                      ]
                    }
                  />
                  <span className="ph-card-copy">
                    <span className="ph-card-title">
                      <strong>{manifest.displayName}</strong>
                      {installed && (
                        <span className={`ph-card-status ${installed.enabled ? 'enabled' : ''}`}>
                          {t(installed.enabled ? 'plugins.enabled' : 'plugins.disabled')}
                        </span>
                      )}
                    </span>
                    <span>
                      {t('plugins.developer')}
                      {manifest.publisher}
                    </span>
                  </span>
                </button>
                <p>{pluginDescription(manifest, store.registry, store.region)}</p>
                <div className="ph-card-bottom">
                  <span className="ph-muted">{manifest.version}</span>
                  {installed ? (
                    <div className="ph-card-installed">
                      {gt(manifest.version, installed.manifest.version) && (
                        <button
                          className="ph-install"
                          disabled={store.busy}
                          onClick={() => setCatalogCandidate(id)}
                        >
                          {t('plugins.update')}
                        </button>
                      )}
                      <RoutingToggle
                        label={manifest.displayName}
                        hideLabel
                        checked={installed.enabled}
                        pending={store.busy}
                        onChange={(enabled) => void store.enable(id, enabled).catch(() => {})}
                      />
                    </div>
                  ) : (
                    <button
                      className="ph-install"
                      disabled={store.busy}
                      onClick={() => {
                        store.select(id);
                        setCatalogCandidate(id);
                      }}
                    >
                      {t('plugins.install')}
                    </button>
                  )}
                </div>
              </article>
            );
          })}
          {skeleton &&
            Array.from({ length: 3 }, (_, index) => (
              <div className="ph-card-skeleton" key={index} aria-hidden="true">
                <ModelCardSkeleton />
              </div>
            ))}
        </div>
        {filtered.length === 0 && !skeleton && (
          <div className="ph-empty">
            <Puzzle size={28} />
            <p>{t('plugins.empty')}</p>
          </div>
        )}
      </div>
      {active && source && (
        <PluginImportDialog
          kind={source.kind}
          value={source.value}
          onClose={() => setSource(null)}
        />
      )}
      {active && catalogCandidate && (
        <PluginImportDialog
          kind="catalog"
          value={catalogCandidate}
          onClose={() => setCatalogCandidate(null)}
        />
      )}
    </div>
  );
}

export function PluginHubPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const store = usePluginsStore();
  const confirm = useConfirm();
  const installed = store.installed.find((plugin) => plugin.id === store.selected);
  const manifest =
    installed?.manifest ??
    store.registry.plugins.find((plugin) => plugin.id === store.selected)?.releases[0]?.manifest;
  if (!manifest)
    return (
      <aside className="ph-details ph-empty">
        <Puzzle size={28} />
        <p>{t('plugins.select')}</p>
      </aside>
    );
  return (
    <aside className="ph-details">
      <div className="ph-detail-identity">
        <PluginIcon
          clientIcon={
            store.registry.plugins.find((plugin) => plugin.id === store.selected)?.clientIcon
          }
          icon={
            installed?.icon ??
            store.registry.plugins.find((plugin) => plugin.id === store.selected)?.icons?.[
              indexRegion(store.region)
            ]
          }
        />
        <h2>{manifest.displayName}</h2>
        <span>
          {t('plugins.developer')}
          {manifest.publisher}
        </span>
      </div>
      <p className="ph-detail-description">
        {pluginDescription(manifest, store.registry, store.region)}
      </p>
      {installed && (
        <div className="ph-detail-action">
          <button
            className="ph-secondary"
            disabled={store.busy || store.loading}
            onClick={async () => {
              try {
                const result = await store.update(installed.id, (candidate) =>
                  confirm({
                    title: t('plugins.permissions'),
                    message: `${candidate.manifest.displayName}\n${candidate.manifest.echobird.permissions
                      .filter(
                        (permission) =>
                          !installed.manifest.echobird.permissions.includes(permission)
                      )
                      .map((permission) =>
                        t(permissionLabels[permission] ?? 'plugins.noPermissions')
                      )
                      .join('\n')}`,
                    confirmText: t('plugins.update'),
                  })
                );
                if (result !== 'cancelled')
                  showToast(
                    'success',
                    t(result === 'updated' ? 'plugins.updated' : 'settings.latestVersion')
                  );
              } catch (error) {
                showToast('error', String(error));
              }
            }}
          >
            {t(store.busy ? 'plugins.checking' : 'plugins.checkUpdates')}
          </button>
          <button
            className="ph-secondary"
            disabled={store.busy}
            onClick={async () => {
              if (
                await confirm({
                  title: t('plugins.uninstall'),
                  message: manifest.displayName,
                  type: 'danger',
                })
              )
                void store.uninstall(installed.id).catch(() => {});
            }}
          >
            {t('plugins.uninstall')}
          </button>
        </div>
      )}
      <dl>
        <dt>{t('plugins.version')}</dt>
        <dd>{manifest.version}</dd>
        <dt>{t('plugins.source')}</dt>
        <dd>
          {manifest.repository
            .split(/[?#]/)[0]
            .replace(/\/+$/, '')
            .split('/')
            .pop()
            ?.replace(/\.git$/, '')}
        </dd>
        <dt>ID</dt>
        <dd>
          {manifest.publisher}.{manifest.name}
        </dd>
        <dt>{t('plugins.license')}</dt>
        <dd>{manifest.license}</dd>
      </dl>
      {installed && <PluginProductControls key={installed.id} plugin={installed} />}
      {installed?.enabled && <p className="ph-muted">{t('plugins.desktopHint')}</p>}
    </aside>
  );
}
