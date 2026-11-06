import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { X } from 'lucide-react';
import * as api from '../../api/tauri';
import type { LocalTool } from '../../api/types';
import { MiniSelect } from '../../components/MiniSelect';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import { useI18n } from '../../hooks/useI18n';
import type { TKey } from '../../i18n/types';
import { useAppManager } from './context';

const COFFEE_TERMINAL = 'coffee-cli';
const DEFAULT_SETTINGS: api.CliLaunchSettings = {
  preset: '',
  directoryMode: 'ask',
  directory: '',
  terminal: 'auto',
};

const PRESET_LABELS: Record<string, TKey> = {
  '': 'cliLaunch.presetDefault',
  plan: 'cliLaunch.presetPlan',
  acceptEdits: 'cliLaunch.presetAcceptEdits',
  bypass: 'cliLaunch.presetBypass',
  readOnly: 'cliLaunch.presetReadOnly',
  workspace: 'cliLaunch.presetWorkspace',
  fullAccess: 'cliLaunch.presetFullAccess',
};

const TERMINAL_NAMES: Record<string, string> = {
  pwsh: 'PowerShell 7',
  powershell: 'PowerShell 5',
  'git-bash': 'Git Bash',
  iterm2: 'iTerm2',
  ghostty: 'Ghostty',
  kitty: 'kitty',
  wezterm: 'WezTerm',
  alacritty: 'Alacritty',
  'gnome-terminal': 'GNOME Terminal',
  konsole: 'Konsole',
  'xfce4-terminal': 'Xfce Terminal',
  [COFFEE_TERMINAL]: 'Coffee CLI',
};

function FieldLabel({ children }: { children: React.ReactNode }) {
  return <div className="mb-1 block text-xs text-cyber-text-secondary">{children}</div>;
}

/** Saves launch choices only; starting a tool stays with the launch button. */
export function CliLaunchDialog({
  initialTool,
  toolLabel,
  onClose,
}: {
  initialTool: string;
  toolLabel: (tool: LocalTool) => string;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const { detectedTools } = useAppManager();
  const tools = useMemo(
    () => detectedTools.filter((tool) => tool.installed && tool.category === 'CLI Code'),
    [detectedTools]
  );
  const [toolId, setToolId] = useState(initialTool);
  const [drafts, setDrafts] = useState<Record<string, api.CliLaunchSettings> | null>(null);
  const [options, setOptions] = useState<api.CliLaunchOptions | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    Promise.all([api.getCliLaunchSettings(), api.getCliLaunchOptions()])
      .then(([settings, launchOptions]) => {
        if (!mounted.current) return;
        setDrafts(settings);
        setOptions(launchOptions);
      })
      .catch((reason) => {
        if (mounted.current) setError(String(reason));
      });
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  const tool = tools.find((candidate) => candidate.id === toolId);
  const settings = { ...DEFAULT_SETTINGS, ...drafts?.[toolId] };
  const update = (changes: Partial<api.CliLaunchSettings>) =>
    setDrafts((current) => ({ ...current, [toolId]: { ...settings, ...changes } }));

  const presets = options?.presets[toolId] ?? [];
  const commandFromCoffee = settings.terminal === COFFEE_TERMINAL;
  const command =
    presets.find((preset) => preset.id === settings.preset)?.command ?? tool?.startCommand ?? '';
  const terminals = [
    ...(options?.terminals ?? []),
    ...(options?.coffeeTools.includes(toolId) ? [COFFEE_TERMINAL] : []),
  ];
  const terminalLabel = (id: string) => {
    if (id === 'auto') return t('cliLaunch.terminalAuto');
    if (id === 'cmd') return t('cliLaunch.terminalCmd');
    return TERMINAL_NAMES[id] ?? id;
  };

  const chooseDirectory = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === 'string' && picked) {
      update({ directoryMode: 'fixed', directory: picked });
    }
  };
  // The picker opens first so a fixed mode never exists without a folder.
  const changeDirectoryMode = (mode: api.CliLaunchSettings['directoryMode']) => {
    if (mode === 'ask' || settings.directory) update({ directoryMode: mode });
    else void chooseDirectory();
  };

  const save = async () => {
    if (!drafts || saving) return;
    setSaving(true);
    setError('');
    try {
      await api.saveCliLaunchSettings(drafts);
      onClose();
    } catch (reason) {
      if (mounted.current) setError(String(reason));
    } finally {
      if (mounted.current) setSaving(false);
    }
  };

  return createPortal(
    <div className="fixed inset-0 z-[9998] flex items-center justify-center">
      <div className="absolute inset-0" onClick={onClose} />
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="cli-launch-title"
        className="relative w-[450px] max-w-[90vw] rounded-xl border border-cyber-border/30 bg-cyber-surface shadow-2xl"
      >
        <div className="flex items-center justify-between px-6 pb-4 pt-5">
          <div className="flex items-center gap-2">
            <span aria-hidden="true" className="font-mono text-sm text-cyber-text opacity-60">
              &gt;_
            </span>
            <span id="cli-launch-title" className="text-base font-bold text-cyber-text">
              {t('cliLaunch.title')}
            </span>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('btn.close')}
            className="text-cyber-text-secondary transition-colors close-button"
          >
            <X size={18} />
          </button>
        </div>
        <div className="space-y-4 px-6 pb-5">
          <div>
            <FieldLabel>{t('cliLaunch.tool')}</FieldLabel>
            <MiniSelect
              value={toolId}
              onChange={setToolId}
              options={tools.map((candidate) => ({
                id: candidate.id,
                label: toolLabel(candidate),
              }))}
            />
          </div>
          <div>
            <FieldLabel>{t('cliLaunch.command')}</FieldLabel>
            <MiniSelect
              value={commandFromCoffee ? COFFEE_TERMINAL : settings.preset}
              disabled={commandFromCoffee || presets.length === 0}
              onChange={(preset) => update({ preset })}
              options={
                commandFromCoffee
                  ? [{ id: COFFEE_TERMINAL, label: t('cliLaunch.commandFromCoffee') }]
                  : [{ id: '' }, ...presets].map((preset) => ({
                      id: preset.id,
                      label: t(PRESET_LABELS[preset.id] ?? 'cliLaunch.presetDefault'),
                    }))
              }
            />
            {/* Space stays reserved so the centered dialog keeps its height across tools. */}
            <p className="mt-1 h-[2lh] overflow-hidden break-all font-mono text-xs text-cyber-text-secondary">
              {!commandFromCoffee && command}
            </p>
          </div>
          <div className={tool?.noCwdPrompt ? 'invisible' : undefined}>
            <div className="flex items-center justify-between">
              <FieldLabel>{t('cliLaunch.directory')}</FieldLabel>
              <ViewModeTabs
                value={settings.directoryMode}
                onChange={changeDirectoryMode}
                options={[
                  { value: 'ask', label: t('cliLaunch.directoryAsk') },
                  { value: 'fixed', label: t('cliLaunch.directoryFixed') },
                ]}
              />
            </div>
            <div className="mt-2 flex h-5 items-center gap-2">
              {settings.directoryMode === 'fixed' && (
                <>
                  <span className="min-w-0 flex-1 truncate font-mono text-xs text-cyber-text">
                    {settings.directory}
                  </span>
                  <button
                    type="button"
                    onClick={() => void chooseDirectory()}
                    className="flex-shrink-0 text-xs text-cyber-text-secondary transition-colors hover:text-cyber-text"
                  >
                    {t('cliLaunch.directoryChange')}
                  </button>
                </>
              )}
            </div>
          </div>
          <div>
            <FieldLabel>{t('cliLaunch.terminal')}</FieldLabel>
            <MiniSelect
              dropUp
              value={settings.terminal}
              onChange={(terminal) => update({ terminal })}
              options={terminals.map((id) => ({ id, label: terminalLabel(id) }))}
            />
          </div>
          {error && (
            <p role="alert" className="break-words text-xs text-red-400">
              {error}
            </p>
          )}
        </div>
        <div className="flex border-t border-cyber-border">
          <button
            type="button"
            onClick={onClose}
            className="flex-1 rounded-bl-xl border-r border-cyber-border px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary transition-all hover:bg-cyber-elevated hover:text-cyber-text"
          >
            {t('model.escCancel')}
          </button>
          <button
            type="button"
            disabled={!drafts || saving}
            onClick={() => void save()}
            className="flex-1 rounded-br-xl px-4 py-3 text-[14px] font-semibold text-cyber-text transition-all enabled:hover:bg-cyber-text/10 disabled:text-cyber-text-muted"
          >
            {t('model.enterSave')}
          </button>
        </div>
      </div>
    </div>,
    document.body
  );
}
