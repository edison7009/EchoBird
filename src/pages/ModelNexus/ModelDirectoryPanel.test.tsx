import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { DirectoryRow } from '../../components/DirectoryRow';
import * as api from '../../api/tauri';
import { ModelNexusPanel } from './ModelNexus';

const { setNewModelForm, setEditingModelId, setShowAddModelModal } = vi.hoisted(() => ({
  setNewModelForm: vi.fn(),
  setEditingModelId: vi.fn(),
  setShowAddModelModal: vi.fn(),
}));
vi.mock('./context', async (original) => ({
  ...(await original<typeof import('./context')>()),
  useModelNexus: () => ({ setNewModelForm, setEditingModelId, setShowAddModelModal }),
}));
vi.mock('../../api/tauri', () => ({ getModelDirectory: vi.fn() }));
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn(() => Promise.resolve()) }));
vi.mock('../../components', () => ({
  getModelIcon: () => null,
  ModelCard: () => null,
  ModelCardSkeleton: () => null,
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'zh-Hans' }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: vi.fn() }));

let renderer: ReactTestRenderer | undefined;
const button = (label: string) =>
  renderer!.root.find((node) => node.type === 'button' && node.props['aria-label'] === label);
beforeEach(() => {
  vi.clearAllMocks();
  renderer = undefined;
});
afterEach(() => {
  act(() => renderer?.unmount());
});

it.each([
  {
    name: 'Legacy remote',
    url: 'https://fixture.test',
    baseUrl: 'https://api.deepseek.com',
    anthropicUrl: 'https://api.deepseek.com/anthropic',
    region: 'global' as const,
    modelId: 'retired-default',
    modelIds: ['retired-other', 'retired-default'],
  },
  {
    name: 'No presets',
    url: 'https://fixture.test',
    baseUrl: 'https://fixture.test/v1',
    anthropicUrl: '',
    region: 'global' as const,
  },
])('opens $name without choosing a static model ID', async (entry) => {
  vi.mocked(api.getModelDirectory).mockResolvedValue({ providers: [entry], relays: [] });
  await act(async () => {
    renderer = create(<ModelNexusPanel />);
  });
  act(() => button(`Add model: ${entry.name}`).props.onClick());
  expect(setNewModelForm).toHaveBeenCalledOnce();
  expect(setNewModelForm.mock.calls[0][0]).toMatchObject({
    name: entry.name,
    baseUrl: entry.baseUrl,
    modelId: '',
    apiKey: '',
  });
  expect(setNewModelForm.mock.calls[0][0].modelIdOptions).toEqual([]);
  expect(setEditingModelId).toHaveBeenCalledExactlyOnceWith(null);
  expect(setShowAddModelModal).toHaveBeenCalledExactlyOnceWith(true);
  expect(api.getModelDirectory).toHaveBeenCalledOnce();
});

it('keeps necessary unsupported-plan suggestions with a remote default, leaving discovery to choose first', async () => {
  const entry = {
    name: 'BytePlus',
    url: 'https://fixture.test',
    baseUrl: 'https://ark.ap-southeast.bytepluses.com/api/coding/v3',
    anthropicUrl: 'https://ark.ap-southeast.bytepluses.com/api/coding',
    region: 'global' as const,
    modelId: 'updated-preset',
    modelIds: ['other-preset', 'updated-preset'],
  };
  vi.mocked(api.getModelDirectory).mockResolvedValue({ providers: [entry], relays: [] });
  await act(async () => {
    renderer = create(<ModelNexusPanel />);
  });
  act(() => button('Add model: BytePlus').props.onClick());
  expect(setNewModelForm.mock.calls[0][0]).toMatchObject({
    modelId: '',
    modelIdOptions: ['updated-preset', 'other-preset'],
  });
});

it('merges the MiniMax editions into one provider with switchable variants', async () => {
  const edition = (name: string, host: string, region: 'cn' | 'global') => ({
    name,
    url: `https://www.${host}`,
    baseUrl: `https://api.${host}/v1`,
    anthropicUrl: `https://api.${host}/anthropic`,
    region,
  });
  vi.mocked(api.getModelDirectory).mockResolvedValue({
    providers: [
      edition('MiniMax EN', 'minimax.io', 'global'),
      edition('Other', 'other.test', 'global'),
      edition('MiniMax CN', 'minimax.cn', 'cn'),
    ],
    relays: [],
  });
  await act(async () => {
    renderer = create(<ModelNexusPanel />);
  });
  const labels = renderer!.root
    .findAll((node) => node.type === 'button')
    .map((node) => node.props['aria-label']);
  expect(labels).toEqual(['btn.addModel', 'Add model: MiniMax', 'Add model: Other']);
  expect(shellOpen).not.toHaveBeenCalled();
  act(() => button('Add model: MiniMax').props.onClick());
  const form = setNewModelForm.mock.calls[0][0];
  // File order, not the UI language, decides the default edition.
  expect(form).toMatchObject({ name: 'MiniMax EN', baseUrl: 'https://api.minimax.io/v1' });
  expect(form.directoryVariants.map((v: { name: string }) => v.name)).toEqual([
    'MiniMax EN',
    'MiniMax CN',
  ]);
});

it('opens a blank add-model form from the first provider tile', async () => {
  vi.mocked(api.getModelDirectory).mockResolvedValue({
    providers: [
      {
        name: 'Provider',
        url: 'https://fixture.test',
        baseUrl: 'https://fixture.test/v1',
        anthropicUrl: '',
        region: 'global' as const,
      },
    ],
    relays: [],
  });
  await act(async () => {
    renderer = create(<ModelNexusPanel />);
  });
  act(() => button('btn.addModel').props.onClick());
  expect(setNewModelForm).toHaveBeenCalledExactlyOnceWith({
    name: '',
    baseUrl: '',
    anthropicUrl: '',
    apiKey: '',
    modelId: '',
  });
  expect(setEditingModelId).toHaveBeenCalledExactlyOnceWith(null);
  expect(setShowAddModelModal).toHaveBeenCalledExactlyOnceWith(true);
});

it('keeps sponsored relays as full cards and lists providers compactly', async () => {
  const entry = (name: string) => ({
    name,
    url: 'https://fixture.test',
    baseUrl: 'https://fixture.test/v1',
    anthropicUrl: '',
    region: 'global' as const,
  });
  vi.mocked(api.getModelDirectory).mockResolvedValue({
    providers: [entry('Provider')],
    relays: [entry('Relay')],
  });
  await act(async () => {
    renderer = create(<ModelNexusPanel />);
  });
  expect(renderer!.root.findAllByType(DirectoryRow).map((row) => row.props.name)).toEqual([
    'Relay',
  ]);
  act(() => button('Add model: Provider').props.onClick());
  expect(setNewModelForm.mock.calls[0][0]).toMatchObject({ name: 'Provider' });
});

it('groups editions by the directory field and labels tabs by edition', async () => {
  const edition = (name: string, host: string, label: string, region: 'cn' | 'global') => ({
    name,
    url: `https://${host}`,
    baseUrl: `https://${host}/v1`,
    anthropicUrl: '',
    region,
    group: 'Fixture Group',
    edition: label,
  });
  vi.mocked(api.getModelDirectory).mockResolvedValue({
    providers: [
      edition('Fixture Intl', 'intl.fixture.test', 'Intl', 'global'),
      edition('Fixture Home', 'home.fixture.test', 'Home', 'cn'),
    ],
    relays: [],
  });
  await act(async () => {
    renderer = create(<ModelNexusPanel />);
  });
  act(() => button('Add model: Fixture Group').props.onClick());
  expect(
    setNewModelForm.mock.calls[0][0].directoryVariants.map((v: { name: string; label: string }) => [
      v.name,
      v.label,
    ])
  ).toEqual([
    ['Fixture Intl', 'Intl'],
    ['Fixture Home', 'Home'],
  ]);
});

it('opens one Sidrune relay before CC Vibe with both address tabs', async () => {
  vi.mocked(api.getModelDirectory).mockResolvedValue(null);
  await act(async () => {
    renderer = create(<ModelNexusPanel />);
  });
  const rows = renderer!.root.findAllByType(DirectoryRow).map((row) => row.props.name);
  expect(rows.slice(0, 2)).toEqual(['Sidrune AI', 'CC Vibe']);
  expect(rows.filter((name) => name.includes('Sidrune'))).toHaveLength(1);
  expect(setNewModelForm).not.toHaveBeenCalled();
  act(() => button('Add model: Sidrune AI').props.onClick());
  expect(setNewModelForm.mock.calls[0][0].directoryVariants).toMatchObject([
    { label: 'cn', baseUrl: 'https://cn.sidrune.ai/v1', anthropicUrl: 'https://cn.sidrune.ai' },
    { label: 'cn2', baseUrl: 'https://cn2.sidrune.ai/v1', anthropicUrl: 'https://cn2.sidrune.ai' },
  ]);
  expect(setShowAddModelModal).toHaveBeenCalledExactlyOnceWith(true);
});
