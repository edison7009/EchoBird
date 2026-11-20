import { enableAccountPlugins } from '../../test/accountPlugins';
import { useLayoutEffect, useState, type ComponentProps } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AccountModelFields } from './AccountModelFields';
import type { NewModelForm } from './context';
import type { AccountProvider } from '../../api/types';
import * as api from '../../api/accountProxy';
import { ModelIdCombobox } from '../../components/ModelIdCombobox';

const { showToast } = vi.hoisted(() => ({ showToast: vi.fn() }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('react-dom', () => ({ createPortal: (children: unknown) => children }));
vi.mock('../../components/ModelIdCombobox', async (original) => {
  const actual = await original<typeof import('../../components/ModelIdCombobox')>();
  return {
    ModelIdCombobox: (props: ComponentProps<typeof actual.ModelIdCombobox>) =>
      props.ariaLabel === 'model.modelId' && (props.optionLabels || props.freeModelIds?.length) ? (
        <actual.ModelIdCombobox {...props} />
      ) : null,
  };
});
vi.mock('../../api/accountProxy', async (original) => ({
  ...(await original<typeof import('../../api/accountProxy')>()),
  listProxyAccounts: vi.fn(),
  getAccountProxyModels: vi.fn(),
  getAccountSpeechModels: vi.fn(),
}));

let renderer: ReactTestRenderer;
let current: NewModelForm;
let stored: Map<string, string>;
let setItem: ReturnType<typeof vi.fn>;
function Harness({
  accountId = '',
  modelId = '',
  active = true,
  provider = 'codex',
  voice = false,
}: {
  accountId?: string;
  modelId?: string;
  active?: boolean;
  provider?: AccountProvider;
  voice?: boolean;
}) {
  const [form, setForm] = useState<NewModelForm>({
    name: 'Codex',
    baseUrl: '',
    anthropicUrl: '',
    apiKey: '',
    modelId,
    accountSource: { provider, accountId },
  });
  useLayoutEffect(() => {
    current = form;
  });
  return (
    <AccountModelFields
      source={form.accountSource!}
      modelId={form.modelId}
      modelIdOptions={form.modelIdOptions}
      setForm={setForm}
      active={active}
      voice={voice}
    />
  );
}

it('uses the saved WorkBuddy speech ID without fetching chat models or querying quota', async () => {
  const accountId = 'a'.repeat(64);
  vi.mocked(api.listProxyAccounts).mockResolvedValue([
    { source: { provider: 'workBuddy', accountId }, label: 'Speech account' },
  ]);
  await act(async () => {
    renderer = create(<Harness provider="workBuddy" voice />);
  });
  expect(current.accountSource).toEqual({ provider: 'workBuddy', accountId });
  expect(current.modelId).toBe(`workbuddy:${accountId}`);
  expect(field('accountProxy.provider').props.options).toEqual([
    'claudeCode',
    'codex',
    'workBuddy',
    'workBuddyAi',
    'dumate',
    'doubaowork',
    'antigravity',
    'trae',
    'traecn',
    'minimax',
    'qoder',
    'qoderCn',
    'kilo',
    'codeArts',
    'devin',
    'cursor',
    'grokbot',
    'qwenworkcn',
    'qwenwork',
  ]);
  expect(field('model.modelId').props.onRefresh).toBeUndefined();
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(api.getAccountSpeechModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
});

it.each(['qwenworkcn', 'qwenwork'] as const)(
  'reuses saved %s speech identity and plan without discovering text models',
  async (provider) => {
    const accountId = 'b'.repeat(64);
    vi.mocked(api.listProxyAccounts).mockResolvedValue([
      {
        source: { provider, accountId },
        label: 'Saved user · Organization',
        plan: 'qwen-office-pro',
      },
    ]);
    await act(async () => {
      renderer = create(<Harness provider={provider} voice />);
    });
    expect(current.name).toBe('Saved user · Organization');
    expect(current.modelId).toBe(`${provider}:${accountId}`);
    expect(field('accountProxy.account').props.optionLabels[accountId]).toBe(
      `Saved user · Organization · ${provider === 'qwenwork' ? 'Pro' : 'agent.qwenworkPro'}`
    );
    expect(api.getAccountProxyModels).not.toHaveBeenCalled();
    expect(api.getAccountSpeechModels).not.toHaveBeenCalled();
    expect(setItem).not.toHaveBeenCalled();
  }
);

it('discovers Kilo speech models and offers explicit refresh without writing text cache', async () => {
  const accountId = 'a'.repeat(64);
  const source = { provider: 'kilo' as const, accountId };
  const selector = `kilo:${accountId}:speech-model`;
  vi.mocked(api.listProxyAccounts).mockResolvedValue([{ source, label: 'Speech account' }]);
  vi.mocked(api.getAccountSpeechModels).mockResolvedValue([selector]);
  api.cacheAccountProxyModels(source, ['text-model']);
  setItem.mockClear();
  await act(async () => {
    renderer = create(<Harness provider="kilo" voice />);
  });
  expect(api.getAccountSpeechModels).toHaveBeenCalledExactlyOnceWith(source);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(current.modelId).toBe(selector);
  expect(api.getCachedAccountProxyModels(source)).toEqual(['text-model']);
  expect(setItem).not.toHaveBeenCalled();
  await act(async () => field('model.modelId').props.onRefresh());
  expect(api.getAccountSpeechModels).toHaveBeenCalledTimes(2);
});

async function open(accountId = '', modelId = '', provider: AccountProvider = 'codex') {
  await act(async () => {
    renderer = create(<Harness accountId={accountId} modelId={modelId} provider={provider} />, {
      createNodeMock: (element) =>
        element.type === 'input'
          ? {
              blur: vi.fn(),
              getBoundingClientRect: () => ({ left: 12, width: 240, top: 100, bottom: 130 }),
            }
          : null,
    });
  });
}
function field(label: string) {
  return renderer.root
    .findAllByType(ModelIdCombobox)
    .find((node) => node.props.ariaLabel === label)!;
}
beforeEach(() => {
  enableAccountPlugins();
  stored = new Map();
  setItem = vi.fn((key: string, value: string) => stored.set(key, value));
  vi.stubGlobal('localStorage', { getItem: (key: string) => stored.get(key) ?? null, setItem });
  vi.stubGlobal('window', {
    innerHeight: 800,
    innerWidth: 1000,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  vi.stubGlobal('document', {
    body: {},
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  vi.mocked(api.listProxyAccounts)
    .mockReset()
    .mockResolvedValue([
      { source: { provider: 'codex', accountId: 'first' }, label: 'First' },
      { source: { provider: 'codex', accountId: 'second' }, label: 'Second' },
    ]);
  vi.mocked(api.getAccountProxyModels).mockReset().mockResolvedValue(['gpt-fixture', 'gpt-other']);
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it('defaults to the first saved account and automatically reads and caches its model directory', async () => {
  await open();
  expect(api.listProxyAccounts).toHaveBeenCalledWith('codex');
  expect(current.accountSource).toEqual({ provider: 'codex', accountId: 'first' });
  expect(field('accountProxy.account').props.value).toBe('first');
  expect(api.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith({
    provider: 'codex',
    accountId: 'first',
  });
  expect(current.modelIdOptions).toEqual(['gpt-fixture', 'gpt-other']);
  expect(current.modelId).toBe('gpt-fixture');
  expect(current.apiKey).toBe('');
  expect(current.name).toBe('First');
  expect(api.getCachedAccountProxyModels(current.accountSource!)).toEqual(current.modelIdOptions);
  expect(setItem).toHaveBeenCalledOnce();
  expect(renderer.root.findAllByType('button')).toHaveLength(0);
  expect(field('model.modelId').props).toMatchObject({
    selectOnly: true,
    refreshLabel: 'btn.refresh',
  });
});

it.each(['trae', 'traecn', 'kiro'] as const)(
  'defaults to the first saved %s identity and plan, then caches its model directory',
  async (provider) => {
    const source = { provider, accountId: 'first' };
    const savedLabel =
      provider === 'kiro'
        ? 'Kiro identity'
        : provider === 'trae'
          ? 'Global identity'
          : 'CN identity';
    const plan = provider === 'kiro' ? 'Pro+' : 'pro';
    vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([
      { source, label: savedLabel, plan },
      { source: { provider, accountId: 'second' }, label: 'Other identity', plan: 'free' },
    ]);
    if (provider === 'kiro')
      vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([
        { source, label: savedLabel, plan, modelLabels: { 'kiro-model': 'Claude Sonnet Fixture' } },
        { source: { provider, accountId: 'second' }, label: 'Other identity', plan: 'free' },
      ]);
    vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce([`${provider}-model`]);
    await open('', '', provider);
    expect(api.listProxyAccounts).toHaveBeenCalledTimes(provider === 'kiro' ? 2 : 1);
    expect(api.listProxyAccounts).toHaveBeenCalledWith(provider);
    expect(current.accountSource).toEqual(source);
    expect(current.name).toBe(savedLabel);
    expect(current.apiKey).toBe('');
    expect(field('accountProxy.account').props).toMatchObject({
      value: 'first',
      optionLabels: {
        first: `${savedLabel} · ${provider === 'kiro' ? 'Pro+' : 'Pro'}`,
        second: 'Other identity · Free',
      },
    });
    expect(api.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source);
    expect(current.modelIdOptions).toEqual([`${provider}-model`]);
    expect(current.modelId).toBe(`${provider}-model`);
    expect(api.getCachedAccountProxyModels(source)).toEqual([`${provider}-model`]);
    if (provider === 'kiro')
      expect(renderer.root.findByType('input').props.value).toBe('Claude Sonnet Fixture');
  }
);

it('ignores a late global directory after switching to CN with the same saved account ID', async () => {
  let resolve!: (models: string[]) => void;
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) => [
    { source: { provider, accountId: 'shared-id' }, label: `${provider} identity`, plan: 'free' },
  ]);
  vi.mocked(api.getAccountProxyModels)
    .mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      })
    )
    .mockResolvedValueOnce(['cn-model']);
  await open('', '', 'trae');
  expect(current.accountSource).toEqual({ provider: 'trae', accountId: 'shared-id' });
  expect(field('model.modelId').props.disabled).toBe(true);
  await act(async () => field('accountProxy.provider').props.onChange('traecn'));
  expect(current.accountSource).toEqual({ provider: 'traecn', accountId: 'shared-id' });
  expect(current.name).toBe('traecn identity');
  expect(current.modelIdOptions).toEqual(['cn-model']);
  expect(current.modelId).toBe('cn-model');
  await act(async () => resolve(['stale-global-model']));
  expect(current.modelIdOptions).toEqual(['cn-model']);
  expect(
    api.getCachedAccountProxyModels({ provider: 'trae', accountId: 'shared-id' })
  ).toBeUndefined();
  expect(api.getCachedAccountProxyModels({ provider: 'traecn', accountId: 'shared-id' })).toEqual([
    'cn-model',
  ]);
  expect(api.getAccountProxyModels).toHaveBeenCalledTimes(2);
  expect(showToast).not.toHaveBeenCalled();
});

it('uses the first saved account cache without another directory request', async () => {
  api.cacheAccountProxyModels({ provider: 'codex', accountId: 'first' }, ['cached-model']);
  setItem.mockClear();
  await open();
  expect(current.accountSource).toEqual({ provider: 'codex', accountId: 'first' });
  expect(current.name).toBe('First');
  expect(current.modelId).toBe('cached-model');
  expect(current.modelIdOptions).toEqual(['cached-model']);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
});

it('preserves an explicitly selected non-first account and its cached model', async () => {
  api.cacheAccountProxyModels({ provider: 'codex', accountId: 'second' }, [
    'second-model',
    'second-other',
  ]);
  setItem.mockClear();
  await open('second', 'second-other');
  expect(current.accountSource).toEqual({ provider: 'codex', accountId: 'second' });
  expect(current.name).toBe('Second');
  expect(current.modelId).toBe('second-other');
  expect(field('accountProxy.account').props.value).toBe('second');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
});

it('keeps an empty selection when the provider has no saved accounts', async () => {
  vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([]);
  await open();
  expect(current.accountSource).toEqual({ provider: 'codex', accountId: '' });
  expect(field('accountProxy.account').props).toMatchObject({ value: '', options: [] });
  expect(field('model.modelId').props.disabled).toBe(true);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
});

it('defaults only for the current provider and ignores a late account list from the previous provider', async () => {
  let resolve!: (accounts: api.ProxyAccount[]) => void;
  vi.mocked(api.listProxyAccounts)
    .mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      })
    )
    .mockResolvedValueOnce([
      { source: { provider: 'kilo', accountId: 'kilo-first' }, label: 'Kilo First' },
    ]);
  api.cacheAccountProxyModels({ provider: 'kilo', accountId: 'kilo-first' }, ['kilo-model']);
  setItem.mockClear();
  await open();
  expect(current.accountSource).toEqual({ provider: 'codex', accountId: '' });
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  await act(async () => field('accountProxy.provider').props.onChange('kilo'));
  expect(current.accountSource).toEqual({ provider: 'kilo', accountId: 'kilo-first' });
  expect(current.name).toBe('Kilo First');
  expect(current.modelId).toBe('kilo-model');
  await act(async () =>
    resolve([{ source: { provider: 'codex', accountId: 'late-codex' }, label: 'Late Codex' }])
  );
  expect(current.accountSource).toEqual({ provider: 'kilo', accountId: 'kilo-first' });
  expect(current.name).toBe('Kilo First');
  expect(field('accountProxy.account').props.options).toEqual(['kilo-first']);
  expect(api.listProxyAccounts).toHaveBeenCalledTimes(2);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
});

it('does not default or fetch models after closing while the saved accounts are still loading', async () => {
  let resolve!: (accounts: api.ProxyAccount[]) => void;
  vi.mocked(api.listProxyAccounts).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  await open();
  await act(async () => renderer.update(<Harness active={false} />));
  const closingForm = current;
  await act(async () =>
    resolve([{ source: { provider: 'codex', accountId: 'late-first' }, label: 'Late First' }])
  );
  expect(current).toBe(closingForm);
  expect(current.accountSource).toEqual({ provider: 'codex', accountId: '' });
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
});

it('reuses the persisted account directory on a new edit form and preserves the selected model', async () => {
  await open('first');
  act(() => renderer.unmount());
  vi.mocked(api.getAccountProxyModels).mockClear();
  setItem.mockClear();
  await open('first', 'gpt-other');
  expect(current.modelIdOptions).toEqual(['gpt-fixture', 'gpt-other']);
  expect(current.modelId).toBe('gpt-other');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
});

it('shows the account label after loading without displaying the internal account ID while pending', async () => {
  let resolve!: (accounts: api.ProxyAccount[]) => void;
  vi.mocked(api.listProxyAccounts).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  await open('first');
  expect(field('accountProxy.account').props).toMatchObject({
    disabled: true,
    value: '',
    placeholder: '—',
  });
  await act(async () =>
    resolve([
      { source: { provider: 'codex', accountId: 'first' }, label: 'First', plan: 'prolite' },
    ])
  );
  expect(field('accountProxy.account').props).toMatchObject({
    disabled: false,
    value: 'first',
    optionLabels: { first: 'First · Pro 100' },
  });
  expect(current.name).toBe('First');
});

it('isolates account and provider caches and restores them without another request', async () => {
  await open('first');
  vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(['second-model']);
  await act(async () => field('accountProxy.account').props.onChange('second'));
  expect(current.modelId).toBe('second-model');
  expect(current.name).toBe('Second');
  await act(async () => field('accountProxy.account').props.onChange('first'));
  expect(current.modelIdOptions).toEqual(['gpt-fixture', 'gpt-other']);
  expect(current.name).toBe('First');
  await act(async () => field('accountProxy.provider').props.onChange('kilo'));
  expect(current.modelIdOptions).toEqual([]);
  expect(current.accountSource).toEqual({ provider: 'kilo', accountId: '' });
  expect(api.getAccountProxyModels).toHaveBeenCalledTimes(2);
  vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(['kilo-model']);
  await act(async () => field('accountProxy.account').props.onChange('first'));
  expect(current.modelIdOptions).toEqual(['kilo-model']);
  expect(api.getCachedAccountProxyModels({ provider: 'codex', accountId: 'first' })).toEqual([
    'gpt-fixture',
    'gpt-other',
  ]);
  expect(api.getCachedAccountProxyModels({ provider: 'kilo', accountId: 'first' })).toEqual([
    'kilo-model',
  ]);
});

it('refreshes explicitly, preserves a valid choice, and retains the snapshot when refresh fails', async () => {
  await open('first');
  act(() => field('model.modelId').props.onChange('gpt-other'));
  vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(['gpt-other', 'new-model']);
  await act(async () => field('model.modelId').props.onRefresh());
  expect(current.modelId).toBe('gpt-other');
  expect(current.modelIdOptions).toEqual(['gpt-other', 'new-model']);
  vi.mocked(api.getAccountProxyModels).mockRejectedValueOnce(new Error('Directory unavailable'));
  setItem.mockClear();
  await act(async () => field('model.modelId').props.onRefresh());
  expect(current.modelId).toBe('gpt-other');
  expect(current.modelIdOptions).toEqual(['gpt-other', 'new-model']);
  expect(api.getCachedAccountProxyModels(current.accountSource!)).toEqual([
    'gpt-other',
    'new-model',
  ]);
  expect(setItem).not.toHaveBeenCalled();
  expect(showToast).toHaveBeenCalledWith('error', 'Error: Directory unavailable');
});

it.each(['workBuddy', 'workBuddyAi', 'qoder', 'qoderCn', 'kilo', 'cline', 'zcode'] as const)(
  'updates %s free metadata with the model directory and preserves it on failure',
  async (provider) => {
    const source = { provider, accountId: 'saved' };
    const saved = { source, label: 'Saved identity', freeModelIds: ['hy3'] };
    api.cacheAccountProxyModels(source, ['auto', 'hy3', 'charged']);
    vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([saved]);
    await open('saved', 'auto', provider);
    expect(field('model.modelId').props.freeModelIds).toEqual(['hy3']);
    expect(api.getAccountProxyModels).not.toHaveBeenCalled();
    vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(['auto', 'hy3', 'charged']);
    vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([{ ...saved, freeModelIds: [] }]);
    await act(async () => field('model.modelId').props.onRefresh());
    expect(field('model.modelId').props.freeModelIds).toEqual([]);
    expect(current.modelId).toBe('auto');
    expect(api.listProxyAccounts).toHaveBeenCalledTimes(2);
    vi.mocked(api.getAccountProxyModels).mockRejectedValueOnce(new Error('Directory unavailable'));
    await act(async () => field('model.modelId').props.onRefresh());
    expect(field('model.modelId').props.freeModelIds).toEqual([]);
    expect(current.modelIdOptions).toEqual(['auto', 'hy3', 'charged']);
    expect(api.listProxyAccounts).toHaveBeenCalledTimes(2);
  }
);

it('ignores old responses when changing accounts or exiting and prevents duplicate refreshes', async () => {
  let resolve!: (models: string[]) => void;
  vi.mocked(api.getAccountProxyModels).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  await open('first');
  expect(field('model.modelId').props.disabled).toBe(true);
  act(() => {
    field('model.modelId').props.onRefresh();
    field('model.modelId').props.onRefresh();
  });
  expect(api.getAccountProxyModels).toHaveBeenCalledOnce();
  await act(async () => field('accountProxy.account').props.onChange('second'));
  await act(async () => resolve(['old-account-model']));
  expect(current.accountSource?.accountId).toBe('second');
  expect(current.modelIdOptions).toEqual(['gpt-fixture', 'gpt-other']);
  expect(
    api.getCachedAccountProxyModels({ provider: 'codex', accountId: 'first' })
  ).toBeUndefined();
  vi.mocked(api.getAccountProxyModels).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  act(() => {
    field('model.modelId').props.onRefresh();
    renderer.unmount();
  });
  setItem.mockClear();
  await act(async () => resolve(['late-model']));
  expect(setItem).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
});

it('caches a successful empty directory and retries it only on refresh', async () => {
  vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce([]);
  await open('first');
  expect(current.modelId).toBe('');
  expect(current.modelIdOptions).toEqual([]);
  act(() => renderer.unmount());
  vi.mocked(api.getAccountProxyModels).mockClear();
  await open('first');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  await act(async () => field('model.modelId').props.onRefresh());
  expect(current.modelId).toBe('gpt-fixture');
});

it('recovers a damaged cache without persisting account credentials', async () => {
  const source = { provider: 'codex' as const, accountId: 'first' };
  api.cacheAccountProxyModels(source, ['old-model']);
  const key = [...stored.keys()][0];
  stored.set(key, '{invalid');
  expect(api.getCachedAccountProxyModels(source)).toBeUndefined();
  await open('first');
  expect(current.modelIdOptions).toEqual(['gpt-fixture', 'gpt-other']);
  expect([...stored.values()]).toEqual([JSON.stringify(['gpt-fixture', 'gpt-other'])]);
});

it('MiniMax distinguishes regions with the same identity and keeps each account model directory cached', async () => {
  const cn = {
    source: { provider: 'minimax' as const, accountId: 'cn' },
    label: 'same@example.test',
    region: 'cn' as const,
  };
  const global = {
    ...cn,
    source: { provider: 'minimax' as const, accountId: 'global' },
    region: 'global' as const,
  };
  vi.mocked(api.listProxyAccounts).mockResolvedValue([cn, global]);
  api.cacheAccountProxyModels(cn.source, ['cn-model']);
  api.cacheAccountProxyModels(global.source, ['global-model']);
  await open('', '', 'minimax');
  expect(current.accountSource).toEqual(cn.source);
  expect(current.modelId).toBe('cn-model');
  expect(field('accountProxy.account').props.optionLabels).toEqual({
    cn: 'agent.minimaxRegionCn · same@example.test',
    global: 'agent.minimaxRegionGlobal · same@example.test',
  });
  await act(async () => field('accountProxy.account').props.onChange('global'));
  expect(current.modelId).toBe('global-model');
  expect(current.name).toBe('agent.minimaxRegionGlobal · same@example.test');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
});

it('Qoder editions isolate account directories even when saved IDs match', async () => {
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) => [
    {
      source: { provider, accountId: 'same' },
      label: provider,
      plan: 'free',
      modelLabels: { [provider === 'qoder' ? 'global-model' : 'cn-model']: `${provider} visible` },
    },
  ]);
  api.cacheAccountProxyModels({ provider: 'qoder', accountId: 'same' }, ['global-model']);
  api.cacheAccountProxyModels({ provider: 'qoderCn', accountId: 'same' }, ['cn-model']);
  await open('', '', 'qoder');
  expect(current.modelId).toBe('global-model');
  expect(renderer.root.findByType('input').props.value).toBe('qoder visible');
  expect(field('accountProxy.account').props.optionLabels.same).toBe('qoder · Free');
  await act(async () => field('accountProxy.provider').props.onChange('qoderCn'));
  expect(current.accountSource).toEqual({ provider: 'qoderCn', accountId: 'same' });
  expect(current.modelId).toBe('cn-model');
  expect(renderer.root.findByType('input').props.value).toBe('qoderCn visible');
  expect(field('accountProxy.account').props.optionLabels.same).toBe('qoderCn · agent.qoderTrial');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
});

it('renders an old Qoder native selection through cached labels without fetching or changing stored IDs', async () => {
  let resolve!: (rows: api.ProxyAccount[]) => void;
  vi.mocked(api.listProxyAccounts).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    })
  );
  const source = { provider: 'qoder' as const, accountId: 'saved' };
  api.cacheAccountProxyModels(source, ['Cantus', 'Qwen3.8-Max']);
  setItem.mockClear();
  await open('saved', 'cmodel', 'qoder');
  expect(current.modelId).toBe('cmodel');
  expect(field('model.modelId').props.disabled).toBe(true);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  await act(async () =>
    resolve([{ source, label: 'Qoder identity', modelLabels: { cmodel: 'Cantus' } }])
  );
  expect(current.modelId).toBe('cmodel');
  expect(current.modelIdOptions).toEqual(['cmodel', 'Qwen3.8-Max']);
  expect(renderer.root.findByType('input').props.value).toBe('Cantus');
  act(() => renderer.root.findByType('input').props.onFocus());
  const selected = renderer.root.find(
    (node) => node.type === 'div' && node.props.onMouseDown && node.props['data-selected']
  );
  expect(selected.findByType('span').children).toEqual(['Cantus']);
  await act(async () => selected.props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(current.modelId).toBe('cmodel');
  expect(api.getCachedAccountProxyModels(source)).toEqual(['Cantus', 'Qwen3.8-Max']);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
});

it.each(['doubaowork', 'dola'] as const)(
  'waits for %s aliases before applying a cached directory and preserves the native model on refresh',
  async (provider) => {
    let resolve!: (rows: api.ProxyAccount[]) => void;
    vi.mocked(api.listProxyAccounts).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      })
    );
    const source = { provider, accountId: 'saved' };
    api.cacheAccountProxyModels(source, ['Pro', 'Turbo']);
    await open('saved', '4', provider);
    expect(current.modelId).toBe('4');
    expect(api.getAccountProxyModels).not.toHaveBeenCalled();
    await act(async () =>
      resolve([{ source, label: 'Saved', modelLabels: { '4': 'Turbo', '5': 'Pro' } }])
    );
    expect(current.modelId).toBe('4');
    expect(current.modelIdOptions).toEqual(['Pro', '4']);
    vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(['Pro', 'Turbo updated']);
    vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([
      { source, label: 'Saved', modelLabels: { '4': 'Turbo updated', '5': 'Pro' } },
    ]);
    await act(async () => field('model.modelId').props.onRefresh());
    expect(current.modelId).toBe('4');
    expect(current.modelIdOptions).toEqual(['Pro', '4']);
    expect(renderer.root.findByType('input').props.value).toBe('Turbo updated');
    expect(api.listProxyAccounts).toHaveBeenCalledTimes(2);
  }
);

it.each(['qoder', 'kiro'] as const)(
  'refreshes %s labels from local metadata without rewriting an equivalent native selection',
  async (provider) => {
    const source = { provider, accountId: 'saved' };
    const nativeId = provider === 'qoder' ? 'cmodel' : 'native-kiro-model';
    api.cacheAccountProxyModels(source, [nativeId, 'other']);
    vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([
      { source, label: 'Saved identity', modelLabels: { [nativeId]: 'Catalog model' } },
    ]);
    await open('saved', nativeId, provider);
    expect(renderer.root.findByType('input').props.value).toBe('Catalog model');
    expect(api.getAccountProxyModels).not.toHaveBeenCalled();
    const models = [provider === 'qoder' ? 'Updated catalog model' : nativeId, 'other-model'];
    vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(models);
    vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([
      { source, label: 'Saved identity', modelLabels: { [nativeId]: 'Updated catalog model' } },
    ]);
    setItem.mockClear();
    await act(async () => field('model.modelId').props.onRefresh());
    expect(current.modelId).toBe(nativeId);
    expect(current.modelIdOptions).toEqual([nativeId, 'other-model']);
    expect(renderer.root.findByType('input').props.value).toBe('Updated catalog model');
    expect(api.getCachedAccountProxyModels(source)).toEqual(models);
    expect(api.getAccountProxyModels).toHaveBeenCalledExactlyOnceWith(source);
    expect(api.listProxyAccounts).toHaveBeenCalledTimes(2);
    expect(setItem).toHaveBeenCalledOnce();
    expect(showToast).not.toHaveBeenCalled();
  }
);

it('ignores late Qoder label rereads after switching editions and preserves snapshots on local read failure', async () => {
  const global = { provider: 'qoder' as const, accountId: 'shared' };
  const cn = { provider: 'qoderCn' as const, accountId: 'shared' };
  api.cacheAccountProxyModels(global, ['cmodel', 'global-other']);
  api.cacheAccountProxyModels(cn, ['cn-native']);
  vi.mocked(api.listProxyAccounts).mockResolvedValueOnce([
    { source: global, label: 'Global identity', modelLabels: { cmodel: 'Cantus' } },
  ]);
  await open('shared', 'cmodel', 'qoder');
  let resolve!: (rows: api.ProxyAccount[]) => void;
  vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(['New Global Model']);
  vi.mocked(api.listProxyAccounts)
    .mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      })
    )
    .mockResolvedValueOnce([
      { source: cn, label: 'CN identity', modelLabels: { 'cn-native': 'CN Model' } },
    ]);
  await act(async () => {
    void field('model.modelId').props.onRefresh();
  });
  await act(async () => field('accountProxy.provider').props.onChange('qoderCn'));
  setItem.mockClear();
  await act(async () =>
    resolve([{ source: global, label: 'Old global', modelLabels: { cmodel: 'New Global Model' } }])
  );
  expect(current.accountSource).toEqual(cn);
  expect(current.modelId).toBe('cn-native');
  expect(renderer.root.findByType('input').props.value).toBe('CN Model');
  expect(api.getCachedAccountProxyModels(global)).toEqual(['cmodel', 'global-other']);
  expect(setItem).not.toHaveBeenCalled();
  vi.mocked(api.getAccountProxyModels).mockResolvedValueOnce(['Changed CN Model']);
  vi.mocked(api.listProxyAccounts).mockRejectedValueOnce(new Error('Local metadata unavailable'));
  const quiet = vi.spyOn(console, 'error').mockImplementation(() => undefined);
  await act(async () => field('model.modelId').props.onRefresh());
  quiet.mockRestore();
  expect(current.modelIdOptions).toEqual(['cn-native']);
  expect(renderer.root.findByType('input').props.value).toBe('CN Model');
  expect(api.getCachedAccountProxyModels(cn)).toEqual(['cn-native']);
  expect(setItem).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
});
