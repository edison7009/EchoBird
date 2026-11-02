import React from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { CliLaunchDialog } from './CliLaunchDialog';

const dialog = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock('react-dom', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-dom')>()),
  createPortal: (node: React.ReactNode) => node,
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: dialog.open }));
vi.mock('../../api/tauri', () => ({
  getCliLaunchSettings: vi.fn(),
  getCliLaunchOptions: vi.fn(),
  saveCliLaunchSettings: vi.fn(),
  startTool: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'en' }),
}));
vi.mock('./context', () => ({
  useAppManager: () => ({
    detectedTools: [
      { id: 'claudecode', name: 'Claude Code', installed: true, category: 'CLI Code' },
      {
        id: 'opencode',
        name: 'OpenCode',
        installed: true,
        category: 'CLI Code',
        startCommand: 'opencode',
      },
      { id: 'aider', name: 'Aider', installed: true, category: 'CLI Code', startCommand: 'aider' },
      {
        id: 'openclaw',
        name: 'OpenClaw',
        installed: true,
        category: 'CLI Code',
        noCwdPrompt: true,
      },
      { id: 'codex', name: 'Codex', installed: false, category: 'CLI Code' },
      { id: 'claudedesktop', name: 'Claude', installed: true, category: 'Desktop' },
    ],
  }),
}));

const options: api.CliLaunchOptions = {
  terminals: ['auto', 'pwsh', 'cmd'],
  presets: {
    claudecode: [{ id: 'bypass', command: 'claude --dangerously-skip-permissions' }],
    opencode: [{ id: 'bypass', command: 'opencode --auto' }],
  },
  coffeeTools: ['claudecode', 'opencode'],
};

const text = (node: ReactTestInstance): string =>
  node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
const buttons = (root: ReactTestRenderer) => root.root.findAllByType('button');
const button = (root: ReactTestRenderer, label: string) => {
  const match = buttons(root).find((candidate) => text(candidate).trim() === label);
  if (!match) throw new Error(`No button: ${label}`);
  return match;
};
const click = (node: ReactTestInstance) => act(async () => node.props.onClick());
const choose = async (root: ReactTestRenderer, current: string, next: string) => {
  await click(button(root, current));
  const option = root.root
    .findAll((node) => typeof node.type === 'string' && node.props.onClick && text(node) === next)
    .pop();
  if (!option) throw new Error(`No option: ${next}`);
  await act(async () => option.props.onClick());
};
const saved = () => vi.mocked(api.saveCliLaunchSettings).mock.calls[0][0];

async function render(initialTool = 'claudecode', onClose = vi.fn()) {
  let root!: ReactTestRenderer;
  await act(async () => {
    root = create(
      <CliLaunchDialog
        initialTool={initialTool}
        toolLabel={(tool) => tool.name}
        onClose={onClose}
      />
    );
  });
  return root;
}

describe('CliLaunchDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    const listeners = { addEventListener: vi.fn(), removeEventListener: vi.fn() };
    vi.stubGlobal('window', listeners);
    vi.stubGlobal('document', { ...listeners, body: {} });
    vi.mocked(api.getCliLaunchSettings).mockResolvedValue({});
    vi.mocked(api.getCliLaunchOptions).mockResolvedValue(options);
    vi.mocked(api.saveCliLaunchSettings).mockResolvedValue();
  });

  it('lists only installed CLI tools', async () => {
    const root = await render();
    await click(button(root, 'Claude Code'));
    const labels = root.root
      .findAll((node) => node.type === 'div' && typeof node.props.onClick === 'function')
      .map(text);
    expect(labels).toEqual(expect.arrayContaining(['OpenCode', 'Aider', 'OpenClaw']));
    expect(labels).not.toContain('Codex');
    expect(labels).not.toContain('Claude');
  });

  it('saves choices for each tool without launching or applying anything', async () => {
    const onClose = vi.fn();
    const root = await render('claudecode', onClose);
    await choose(root, 'cliLaunch.presetDefault', 'cliLaunch.presetBypass');
    expect(text(root.root)).toContain('claude --dangerously-skip-permissions');
    await choose(root, 'Claude Code', 'OpenCode');
    await choose(root, 'cliLaunch.terminalAuto', 'PowerShell 7');
    await click(button(root, 'model.enterSave'));

    expect(saved()).toEqual({
      claudecode: { preset: 'bypass', directoryMode: 'ask', directory: '', terminal: 'auto' },
      opencode: { preset: '', directoryMode: 'ask', directory: '', terminal: 'pwsh' },
    });
    expect(onClose).toHaveBeenCalledOnce();
    expect(api.startTool).not.toHaveBeenCalled();
  });

  it('discards edits on cancel', async () => {
    const onClose = vi.fn();
    const root = await render('claudecode', onClose);
    await choose(root, 'cliLaunch.presetDefault', 'cliLaunch.presetBypass');
    await click(button(root, 'model.escCancel'));
    expect(onClose).toHaveBeenCalledOnce();
    expect(api.saveCliLaunchSettings).not.toHaveBeenCalled();
  });

  it('enters fixed mode only after a folder is picked', async () => {
    dialog.open.mockResolvedValueOnce(null).mockResolvedValueOnce('D:/work');
    const root = await render();
    await click(button(root, 'cliLaunch.directoryFixed'));
    expect(text(root.root)).not.toContain('cliLaunch.directoryChange');

    await click(button(root, 'cliLaunch.directoryFixed'));
    expect(text(root.root)).toContain('D:/work');
    await click(button(root, 'model.enterSave'));
    expect(saved().claudecode).toMatchObject({ directoryMode: 'fixed', directory: 'D:/work' });
  });

  it('keeps the folder when switching back to asking', async () => {
    vi.mocked(api.getCliLaunchSettings).mockResolvedValue({
      claudecode: { preset: '', directoryMode: 'fixed', directory: 'D:/work', terminal: 'auto' },
    });
    const root = await render();
    await click(button(root, 'cliLaunch.directoryAsk'));
    await click(button(root, 'model.enterSave'));
    expect(saved().claudecode).toMatchObject({ directoryMode: 'ask', directory: 'D:/work' });
    expect(dialog.open).not.toHaveBeenCalled();
  });

  it('offers Coffee CLI only for tools it can open and leaves commands to it', async () => {
    const root = await render();
    await choose(root, 'cliLaunch.terminalAuto', 'Coffee CLI');
    const command = button(root, 'cliLaunch.commandFromCoffee');
    expect(command).toBeTruthy();
    await click(command);
    expect(text(root.root)).not.toContain('cliLaunch.presetBypass');

    await choose(root, 'Claude Code', 'Aider');
    await click(button(root, 'cliLaunch.terminalAuto'));
    expect(text(root.root)).not.toContain('Coffee CLI');
  });

  it('hides the folder choice but keeps its space for tools that do not use one', async () => {
    const hidden = (root: ReactTestRenderer) =>
      root.root.findAll((node) => node.type === 'div' && node.props.className === 'invisible');
    expect(hidden(await render('claudecode'))).toHaveLength(0);
    const [section] = hidden(await render('openclaw'));
    expect(text(section)).toContain('cliLaunch.directory');
  });

  it('keeps the dialog open and shows why when saving fails', async () => {
    const onClose = vi.fn();
    vi.mocked(api.saveCliLaunchSettings).mockRejectedValue('disk full');
    const root = await render('claudecode', onClose);
    await click(button(root, 'model.enterSave'));
    expect(text(root.root)).toContain('disk full');
    expect(onClose).not.toHaveBeenCalled();
  });
});
