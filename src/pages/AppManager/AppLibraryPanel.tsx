import { Copy, FolderOpen, Plus, Power, PowerOff, Search, Trash2 } from 'lucide-react';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { AppPanelNotice } from './AppPanelNotice';
import { UnderlineTabs } from './UnderlineTabs';
import type { TKey } from '../../i18n';
import type { LocalTool } from '../../api/types';
import {
  appLibrary,
  appLibraryOwner,
  type LibraryCopyResult,
  type LibraryEntry,
  type LibraryKind,
} from '../../api/appLibrary';
import { openFolder } from '../../api/tauri';
import { useConfirm } from '../../components/ConfirmDialog';
import { useI18n } from '../../hooks/useI18n';
import { useAppLibraryStore } from '../../stores/appLibraryStore';
import { useToolsStore } from '../../stores/toolsStore';

type Tab = 'model' | LibraryKind;

const toolTabs: Partial<Record<string, readonly LibraryKind[]>> = {
  claudecode: ['skills', 'mcp', 'plugins', 'memory'],
  codex: ['skills', 'mcp', 'plugins', 'memory'],
  claudedesktop: ['skills', 'mcp', 'memory'],
  antigravity: ['skills', 'mcp', 'memory'],
  antigravitydesktop: ['skills', 'mcp', 'memory'],
  dsh: ['skills', 'mcp', 'memory'],
  cursor: ['skills', 'mcp'],
  cline: ['skills', 'mcp', 'memory'],
  devincli: ['skills', 'mcp', 'memory'],
  qoder: ['skills', 'mcp'],
  workbuddy: ['skills', 'mcp'],
  kimicode: ['skills', 'mcp', 'plugins', 'memory'],
  opencode: ['skills', 'mcp', 'plugins', 'memory'],
  pi: ['skills', 'mcp', 'plugins', 'memory'],
};

export function libraryKinds(toolId: string | undefined): readonly LibraryKind[] {
  return (toolId && toolTabs[appLibraryOwner(toolId)]) || [];
}

const labels: Record<Tab, TKey> = {
  model: 'appLibrary.model',
  skills: 'appLibrary.skills',
  mcp: 'appLibrary.mcp',
  plugins: 'appLibrary.plugins',
  memory: 'appLibrary.memory',
};

const statusKeys: Record<string, TKey> = {
  copied: 'appLibrary.copied',
  exists: 'appLibrary.exists',
  unsupported: 'appLibrary.unsupported',
  coworkProfileMissing: 'appLibrary.coworkProfileMissing',
  coworkProfileAmbiguous: 'appLibrary.coworkProfileAmbiguous',
  mcpLocalOnly: 'appLibrary.mcpLocalOnly',
  missing: 'appLibrary.missing',
  invalidConfig: 'appLibrary.invalidConfig',
  invalidName: 'appLibrary.invalidName',
  readFailed: 'appLibrary.readFailed',
  writeFailed: 'appLibrary.writeFailed',
};

function statusKey(status: unknown): TKey {
  return statusKeys[String(status)] ?? 'appLibrary.writeFailed';
}

const inputClass =
  'w-full rounded-md border border-cyber-border bg-cyber-bg/30 px-2.5 py-1.5 text-xs text-cyber-text placeholder:text-cyber-text-secondary focus-visible:outline focus-visible:outline-2 focus-visible:outline-cyber-accent';
const buttonClass =
  'min-h-7 rounded-md bg-cyber-elevated/60 px-2.5 text-xs font-semibold text-cyber-text hover:bg-cyber-elevated disabled:opacity-45';
const iconClass =
  'account-icon-button flex h-7 w-7 items-center justify-center rounded text-cyber-text-secondary hover:bg-cyber-elevated/70 hover:text-cyber-text disabled:opacity-45';

export function AppLibraryTabs({
  kinds,
  active,
  onSelect,
}: {
  kinds: readonly LibraryKind[];
  active: Tab;
  onSelect: (kind: Tab) => void;
}) {
  const { t } = useI18n();
  const items = useMemo(
    () => (['model', ...kinds] as Tab[]).map((id) => ({ id, label: t(labels[id]) })),
    [kinds, t]
  );
  return (
    <UnderlineTabs
      items={items}
      active={active}
      onSelect={onSelect}
      label={t('appLibrary.title')}
      className="mx-2 mb-2"
      compact
    />
  );
}

function CopyDialog({
  kind,
  source,
  entry,
  onClose,
}: {
  kind: LibraryKind;
  source: LocalTool;
  entry: LibraryEntry;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const toolList = useToolsStore((state) => state.detectedTools);
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [targets, setTargets] = useState<string[]>([]);
  const [outcomes, setOutcomes] = useState<LibraryCopyResult[] | null>(null);
  const [error, setError] = useState<TKey | null>(null);
  const [busy, setBusy] = useState(false);
  const seen = new Set([appLibraryOwner(source.id, kind)]);
  const candidates = toolList.filter((tool) => {
    const id = appLibraryOwner(tool.id, kind);
    if (seen.has(id) || !tool.installed || !libraryKinds(tool.id).includes(kind)) return false;
    seen.add(id);
    return true;
  });

  useEffect(() => {
    const modal = dialog.current;
    modal?.showModal?.();
    return () => modal?.close?.();
  }, []);

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      const results = await appLibrary.copy(source.id, kind, entry.id, targets);
      setOutcomes(results);
      const { snapshots, read } = useAppLibraryStore.getState();
      for (const result of results) {
        if (result.status === 'copied' && snapshots[appLibraryOwner(result.toolId, kind)]?.[kind]) {
          void read(result.toolId, kind, true);
        }
      }
    } catch (reason) {
      setError(statusKey(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog
      ref={dialog}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
      className="m-auto w-80 max-w-[90vw] rounded-xl border border-cyber-border bg-cyber-surface p-3 text-cyber-text shadow-xl backdrop:bg-black/40"
    >
      <h3 id={titleId} className="mb-2 break-words text-sm font-semibold">
        {t('appLibrary.copyTo')} · {entry.name}
      </h3>
      {outcomes ? (
        <ul className="grid gap-1.5" aria-live="polite">
          {outcomes.map((item) => (
            <li
              key={item.toolId}
              className="flex justify-between gap-2 rounded-md bg-cyber-bg/30 px-2 py-1.5 text-xs"
            >
              <span>{candidates.find((tool) => tool.id === item.toolId)?.name ?? item.toolId}</span>
              <span className="min-w-0 text-right text-cyber-text-secondary">
                {t(statusKey(item.status))}
              </span>
            </li>
          ))}
        </ul>
      ) : candidates.length ? (
        <div className="grid max-h-56 gap-1.5 overflow-y-auto">
          {candidates.map((tool) => (
            <label
              key={tool.id}
              className="flex items-center gap-2 rounded-md bg-cyber-bg/30 px-2 py-1.5 text-xs"
            >
              <input
                type="checkbox"
                disabled={busy}
                checked={targets.includes(tool.id)}
                onChange={(event) =>
                  setTargets((selected) =>
                    event.target.checked
                      ? [...selected, tool.id]
                      : selected.filter((id) => id !== tool.id)
                  )
                }
              />
              <span>{tool.name}</span>
            </label>
          ))}
        </div>
      ) : (
        <p className="text-xs text-cyber-text-secondary">{t('appLibrary.noTargets')}</p>
      )}
      {error && (
        <p role="alert" className="mt-2 text-xs text-red-500">
          {t(error)}
        </p>
      )}
      <div className="mt-3 flex justify-end gap-2">
        <button type="button" className={buttonClass} disabled={busy} onClick={onClose}>
          {t(outcomes ? 'btn.close' : 'btn.cancel')}
        </button>
        {!outcomes && (
          <button
            type="button"
            className={buttonClass}
            disabled={busy || !targets.length}
            onClick={() => void submit()}
          >
            {t('appLibrary.copyTo')}
            {targets.length ? ` · ${targets.length}` : ''}
          </button>
        )}
      </div>
    </dialog>
  );
}

function McpForm({
  busy,
  onCancel,
  onSave,
  localOnly,
}: {
  busy: boolean;
  localOnly: boolean;
  onCancel: () => void;
  onSave: (name: string, definition: string) => void;
}) {
  const { t } = useI18n();
  const [name, setName] = useState('');
  const [definition, setDefinition] = useState('');
  return (
    <form
      className="grid gap-2 rounded-lg bg-cyber-surface p-2.5"
      onSubmit={(event) => {
        event.preventDefault();
        onSave(name.trim(), definition.trim());
      }}
    >
      <label className="grid gap-1 text-xs">
        {t('appLibrary.serverName')}
        <input
          className={inputClass}
          value={name}
          maxLength={128}
          required
          onChange={(event) => setName(event.target.value)}
        />
      </label>
      <label className="grid gap-1 text-xs">
        {t('appLibrary.serverConfig')}
        <textarea
          className={`${inputClass} min-h-20 font-mono`}
          value={definition}
          placeholder={
            localOnly
              ? '{"command":"node","args":["/path/server.js"]}'
              : '{"url":"https://example.com/mcp"}'
          }
          required
          onChange={(event) => setDefinition(event.target.value)}
        />
      </label>
      <div className="flex justify-end gap-2">
        <button type="button" className={buttonClass} disabled={busy} onClick={onCancel}>
          {t('btn.cancel')}
        </button>
        <button
          type="submit"
          className={buttonClass}
          disabled={busy || !name.trim() || !definition.trim()}
        >
          {t('btn.save')}
        </button>
      </div>
    </form>
  );
}

export function AppLibraryPanel({
  tool,
  kind,
  revision,
}: {
  tool: LocalTool;
  kind: LibraryKind;
  revision: number;
}) {
  const { t } = useI18n();
  const confirm = useConfirm();
  const snapshot = useAppLibraryStore(
    (state) => state.snapshots[appLibraryOwner(tool.id, kind)]?.[kind]
  );
  const read = useAppLibraryStore((state) => state.read);
  const lastRevision = useRef(revision);
  const [showSkeleton, setShowSkeleton] = useState(false);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<TKey | null>(null);
  const [query, setQuery] = useState('');
  const [copying, setCopying] = useState<LibraryEntry | null>(null);
  const [adding, setAdding] = useState(false);

  const entries = snapshot?.entries ?? [];
  const loading = !snapshot;
  const readStatus = snapshot?.error == null ? undefined : String(snapshot.error);
  const readError =
    readStatus === undefined
      ? undefined
      : readStatus === 'unsupported'
        ? 'appLibrary.readUnsupported'
        : (statusKeys[readStatus] ?? 'appLibrary.readFailed');

  useEffect(() => {
    const manualRefresh = lastRevision.current !== revision;
    lastRevision.current = revision;
    void read(tool.id, kind, manualRefresh);
  }, [tool.id, kind, revision, read]);

  useEffect(() => {
    if (!loading) return;
    const timer = setTimeout(() => setShowSkeleton(true), 120);
    return () => clearTimeout(timer);
  }, [loading]);

  const runAction = async (action: () => Promise<unknown>, update = true) => {
    setBusy(true);
    setActionError(null);
    try {
      await action();
      if (update) await read(tool.id, kind, true);
    } catch (error) {
      setActionError(statusKey(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (item: LibraryEntry) => {
    const accepted = await confirm({
      title: `${t('btn.delete')} · ${item.name}`,
      message: t(kind === 'skills' ? 'appLibrary.archiveSkill' : 'appLibrary.removeMcp'),
      confirmText: t('btn.delete'),
      cancelText: t('btn.cancel'),
      type: 'danger',
    });
    if (accepted) await runAction(() => appLibrary.remove(tool.id, kind, item.id));
  };

  const match = query.trim().toLowerCase();
  const visible = match
    ? entries.filter(
        (item) =>
          item.name.toLowerCase().includes(match) || item.description?.toLowerCase().includes(match)
      )
    : entries;
  return (
    <section
      role="tabpanel"
      aria-label={t(labels[kind])}
      aria-busy={loading || undefined}
      className="flex min-h-full flex-col gap-1.5 text-cyber-text"
    >
      {kind === 'mcp' &&
        (adding && snapshot?.entries ? (
          <McpForm
            busy={busy}
            localOnly={tool.id === 'claudedesktop'}
            onCancel={() => setAdding(false)}
            onSave={(name, definition) =>
              void runAction(async () => {
                await appLibrary.addMcp(tool.id, name, definition);
                setAdding(false);
              })
            }
          />
        ) : (
          <button
            type="button"
            disabled={busy || !snapshot?.entries}
            onClick={() => setAdding(true)}
            className="flex min-h-8 items-center justify-center gap-1.5 rounded-lg border border-dashed border-cyber-border px-2 text-xs text-cyber-text-secondary hover:text-cyber-text disabled:opacity-40"
          >
            <Plus size={13} aria-hidden="true" />
            {t('appLibrary.addMcp')}
          </button>
        ))}
      {kind === 'skills' && entries.length > 0 && (
        <div className="relative mb-1">
          <Search
            size={13}
            aria-hidden="true"
            className="pointer-events-none absolute left-2 top-2 text-cyber-text-secondary"
          />
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('appLibrary.searchSkills')}
            aria-label={t('appLibrary.searchSkills')}
            className={`${inputClass} pl-7`}
          />
        </div>
      )}
      {loading ? (
        showSkeleton && (
          <div aria-hidden="true" className="grid gap-1.5">
            {[0, 1, 2].map((row) => (
              <div
                key={row}
                className="flex min-h-[47px] flex-col justify-center gap-1.5 rounded-lg bg-cyber-surface/60 px-2 py-1.5"
              >
                <div className="h-2 w-2/5 rounded bg-cyber-border/60" />
                <div className="h-2 w-3/5 rounded bg-cyber-border/35" />
              </div>
            ))}
          </div>
        )
      ) : readError && entries.length === 0 ? (
        <AppPanelNotice role="alert">{t(readError)}</AppPanelNotice>
      ) : !visible.length ? (
        <AppPanelNotice>{t(match ? 'appLibrary.noMatches' : 'appLibrary.empty')}</AppPanelNotice>
      ) : (
        visible.map((item, index) => {
          const PowerIcon = item.enabled === false ? PowerOff : Power;
          return (
            <div key={item.id}>
              {item.group !== null && (index === 0 || visible[index - 1].group !== item.group) && (
                <h3 className="mb-1 px-1 pt-1 text-[11px] font-semibold text-cyber-text-secondary">
                  {item.group || t('appLibrary.instructions')}
                </h3>
              )}
              <div className="sidebar-card flex min-h-[47px] items-center gap-2 rounded-lg bg-cyber-surface px-2 py-1.5">
                <div className="min-w-0 grow">
                  <div className="truncate text-xs font-semibold">{item.name}</div>
                  {item.description && (
                    <div className="truncate text-[11px] text-cyber-text-secondary">
                      {item.description}
                    </div>
                  )}
                </div>
                <div className="flex shrink-0 items-center">
                  {item.canToggle && (
                    <button
                      type="button"
                      aria-label={`${t(item.enabled ? 'appLibrary.disable' : 'appLibrary.enable')} · ${item.name}`}
                      aria-pressed={item.enabled ?? undefined}
                      title={t(
                        item.enabled === null
                          ? 'appLibrary.enable'
                          : item.enabled
                            ? 'appLibrary.enabledHint'
                            : 'appLibrary.disabledHint'
                      )}
                      className={iconClass}
                      disabled={busy}
                      onClick={() =>
                        void runAction(() =>
                          appLibrary.toggle(tool.id, kind, item.id, !item.enabled)
                        )
                      }
                    >
                      <PowerIcon
                        size={13}
                        className={item.enabled ? 'text-emerald-600 dark:text-emerald-400' : ''}
                        aria-hidden="true"
                      />
                    </button>
                  )}
                  {item.canCopy && (
                    <button
                      type="button"
                      aria-label={`${t('appLibrary.copyTo')} · ${item.name}`}
                      title={t('appLibrary.copyTo')}
                      className={iconClass}
                      disabled={busy}
                      onClick={() => setCopying(item)}
                    >
                      <Copy size={13} aria-hidden="true" />
                    </button>
                  )}
                  {item.path && (
                    <button
                      type="button"
                      aria-label={`${t('appLibrary.openLocation')} · ${item.name}`}
                      title={t('appLibrary.openLocation')}
                      className={iconClass}
                      disabled={busy}
                      onClick={() => void runAction(() => openFolder(item.path!), false)}
                    >
                      <FolderOpen size={13} aria-hidden="true" />
                    </button>
                  )}
                  {item.canRemove && (
                    <button
                      type="button"
                      aria-label={`${t('btn.delete')} · ${item.name}`}
                      title={t('btn.delete')}
                      className={`${iconClass} hover:text-red-500`}
                      disabled={busy}
                      onClick={() => void remove(item)}
                    >
                      <Trash2 size={13} aria-hidden="true" />
                    </button>
                  )}
                </div>
              </div>
            </div>
          );
        })
      )}
      {readError && entries.length > 0 && (
        <AppPanelNotice role="status" inline>
          {t(readError)}
        </AppPanelNotice>
      )}
      {actionError && (
        <AppPanelNotice role="alert" inline error>
          {t(actionError)}
        </AppPanelNotice>
      )}
      {copying && (
        <CopyDialog kind={kind} source={tool} entry={copying} onClose={() => setCopying(null)} />
      )}
    </section>
  );
}
