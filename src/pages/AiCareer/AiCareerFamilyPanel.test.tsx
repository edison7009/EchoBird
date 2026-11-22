import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import type { AiCareerFamily } from '../../api/aiCareer';
import type { LocalTool } from '../../api/types';
import { useAiCareerStore } from '../../stores/aiCareerStore';
import { useToolsStore } from '../../stores/toolsStore';
import claude from '../../../docs/api/tools/install/claudecode.json';
import codex from '../../../docs/api/tools/install/codex.json';
import deepseek from '../../../docs/api/tools/install/dsh.json';
import mimo from '../../../docs/api/tools/install/mimocode.json';
import hermes from '../../../docs/api/tools/install/hermes.json';
import opencode from '../../../docs/api/tools/install/opencode.json';
import antigravityDesktop from '../../../docs/api/tools/install/antigravitydesktop.json';
import workbuddy from '../../../docs/api/tools/install/workbuddy.json';
import { AiCareerFamilyPanel } from './AiCareerFamilyPanel';

const { showToast } = vi.hoisted(() => ({ showToast: vi.fn() }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: string) => (key === 'accountCenter.website' ? 'Website for {name}' : key),
  }),
}));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../api/tauri', () => ({ openExternal: vi.fn(), scanTools: vi.fn() }));

const families: ReadonlyArray<{
  family: AiCareerFamily;
  name: string;
  catalog: { id: string; displayName: string; homepage: string };
}> = [
  { family: 'claude', name: 'Claude', catalog: claude },
  { family: 'codex', name: 'Codex', catalog: codex },
  { family: 'deepseek', name: 'DeepSeek', catalog: deepseek },
  { family: 'mimo', name: 'MiMo', catalog: mimo },
  { family: 'hermes', name: 'Hermes', catalog: hermes },
  { family: 'opencode', name: 'OpenCode', catalog: opencode },
  { family: 'antigravity', name: 'Antigravity', catalog: antigravityDesktop },
  { family: 'workbuddy', name: 'WorkBuddy', catalog: workbuddy },
];

const tools: LocalTool[] = families.map(({ catalog }) => ({
  id: catalog.id,
  name: catalog.displayName,
  category: 'CLI Code',
  installed: false,
  website: catalog.homepage,
}));

describe('career family website actions', () => {
  let renderer: ReactTestRenderer;
  const mount = () => {
    act(() => {
      renderer = create(<AiCareerFamilyPanel />);
    });
  };
  const websiteButton = (name: string) =>
    renderer.root
      .findAllByType('button')
      .find((button) => button.props['aria-label'] === `Website for ${name}`)!;
  const familyButton = (name: string) =>
    renderer.root
      .findAllByType('button')
      .find((button) => button.findAllByType('img').some((image) => image.props.alt === name))!;

  beforeEach(() => {
    useAiCareerStore.setState({ selectedFamily: null });
    useToolsStore.setState({ detectedTools: tools, isScanning: false });
    vi.mocked(api.openExternal).mockResolvedValue();
    vi.mocked(api.scanTools).mockResolvedValue([]);
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    useToolsStore.setState({ detectedTools: [], isScanning: false });
    useAiCareerStore.setState({ selectedFamily: null });
    vi.resetAllMocks();
  });

  it('reads the catalog passively without scanning tools or opening a website', () => {
    mount();
    expect(families.every(({ name }) => !!websiteButton(name))).toBe(true);
    expect(api.scanTools).not.toHaveBeenCalled();
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(useAiCareerStore.getState().selectedFamily).toBeNull();
  });

  it('groups desktop and CLI editions under their family buttons', () => {
    mount();
    expect(familyButton('Antigravity')).toBeDefined();
    expect(familyButton('WorkBuddy')).toBeDefined();
    expect(familyButton('Antigravity CLI')).toBeUndefined();
    expect(familyButton('WorkBuddy AI')).toBeUndefined();
  });

  it.each(families)('selects $name without opening its website or scanning', ({ family, name }) => {
    mount();
    act(() => familyButton(name).props.onClick());
    expect(useAiCareerStore.getState().selectedFamily).toBe(family);
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(api.scanTools).not.toHaveBeenCalled();
  });

  it.each(families)(
    'opens the catalog website for $name without changing selection',
    async ({ name, catalog }) => {
      useAiCareerStore.setState({ selectedFamily: 'codex' });
      mount();
      await act(async () => websiteButton(name).props.onClick());
      expect(api.openExternal).toHaveBeenCalledOnce();
      expect(api.openExternal).toHaveBeenCalledWith(catalog.homepage);
      expect(useAiCareerStore.getState().selectedFamily).toBe('codex');
      expect(api.scanTools).not.toHaveBeenCalled();
    }
  );

  it('omits the website action when that catalog entry has no URL', () => {
    useToolsStore.setState({
      detectedTools: tools.map((tool) =>
        tool.id === 'claudecode' ? { ...tool, website: undefined } : tool
      ),
    });
    mount();
    expect(websiteButton('Claude')).toBeUndefined();
    expect(familyButton('Claude')).toBeDefined();
    expect(api.openExternal).not.toHaveBeenCalled();
    expect(api.scanTools).not.toHaveBeenCalled();
  });

  it('shows an explicit open failure while keeping the selected family', async () => {
    vi.mocked(api.openExternal).mockRejectedValue(new Error('open failed'));
    useAiCareerStore.setState({ selectedFamily: 'mimo' });
    mount();
    await act(async () => websiteButton('Claude').props.onClick());
    expect(showToast).toHaveBeenCalledWith('error', 'aiCareer.actionFailed');
    expect(useAiCareerStore.getState().selectedFamily).toBe('mimo');
    expect(api.scanTools).not.toHaveBeenCalled();
  });
});
