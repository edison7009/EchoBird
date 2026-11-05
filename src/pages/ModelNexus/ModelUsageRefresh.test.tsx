// Regression coverage for credential changes, shared quota cache and explicit refresh.
import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { AddModelModal, ModelNexusProvider } from './ModelNexus';
import { useModelNexus } from './context';
import type { NewModelForm } from './context';
const { showToast, original } = vi.hoisted(() => {
  vi.stubGlobal('__APP_EDITION__', 'full');
  return {
    showToast: vi.fn(),
    original: {
      internalId: 'model',
      name: 'Test',
      baseUrl: 'https://api.deepseek.com',
      apiKey: 'fake-old-key',
      modelId: 'deepseek-chat',
    },
  };
});
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'en' }),
}));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../components', () => ({
  ModelCard: () => null,
  ModelCardSkeleton: () => null,
  ModelIdCombobox: () => null,
  getModelIcon: () => null,
}));
vi.mock('../FreeModels/provider', () => ({
  useFreeModels: () => ({
    addSelectedModel: vi.fn(),
    updateSelectedModel: vi.fn(),
    selectedIds: new Set(),
  }),
}));
vi.mock('../../api/tauri', () => ({
  getModels: vi.fn(),
  updateModel: vi.fn(),
  deleteModel: vi.fn(),
  queryModelUsage: vi.fn(),
  hasVolcAksk: vi.fn(),
  saveVolcAksk: vi.fn(),
}));
describe('Model usage refresh and credential changes', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useModelNexus>;
  function Harness() {
    const state = useModelNexus();
    useLayoutEffect(() => {
      context = state;
    });
    return <AddModelModal />;
  }
  async function open() {
    await act(async () => {
      renderer = create(
        <ModelNexusProvider>
          <Harness />
        </ModelNexusProvider>
      );
    });
  }
  function balance(value: number): api.ModelUsageData {
    return { quotas: [{ balance: value, balanceUnit: 'CNY', percentage: 0, resetAt: 0 }] };
  }
  async function edit(changes: Partial<NewModelForm>) {
    await act(async () => {
      await context.handleCardEdit(context.userModels[0]);
    });
    act(() => context.setNewModelForm((previous) => ({ ...previous, ...changes })));
    const save = renderer.root
      .findAllByType('button')
      .find((button) => button.children.includes('model.enterSave'))!;
    await act(async () => {
      await save.props.onClick();
    });
  }
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(api.getModels).mockResolvedValue([{ ...original }]);
    vi.mocked(api.updateModel).mockImplementation(async (_id, form) => ({ ...original, ...form }));
    vi.mocked(api.deleteModel).mockResolvedValue(true);
    vi.mocked(api.hasVolcAksk).mockResolvedValue(true);
    vi.mocked(api.saveVolcAksk).mockResolvedValue(undefined);
    vi.mocked(api.queryModelUsage).mockResolvedValue({ success: true, data: balance(17) });
  });
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.clearAllMocks();
  });
  it('positive control: passive load makes no quota request, explicit refresh updates shared state', async () => {
    await open();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    await act(async () => {
      await context.refreshSingleUsage('model');
    });
    expect(api.queryModelUsage).toHaveBeenCalledExactlyOnceWith('model');
    expect(context.modelUsageData.model.quotas[0].balance).toBe(17);
  });
  it('clears previous-account quota after saving a different API key without querying quota', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17) }));
    await edit({ apiKey: 'fake-new-key' });
    expect(api.updateModel).toHaveBeenCalledWith(
      'model',
      expect.objectContaining({ apiKey: 'fake-new-key' })
    );
    expect(context.userModels[0].apiKey).toBe('fake-new-key');
    expect(context.modelUsageData.model).toBeUndefined();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
  });
  it('ignores old in-flight quota after a credential change', async () => {
    await open();
    let resolveOld!: (result: api.UsageResult) => void;
    vi.mocked(api.queryModelUsage).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.refreshSingleUsage('model');
    });
    await edit({ apiKey: 'fake-new-key' });
    act(() => context.setModelUsageData({ model: balance(80) }));
    await act(async () => {
      resolveOld({ success: true, data: balance(17) });
      await pending;
    });
    expect(context.userModels[0].apiKey).toBe('fake-new-key');
    expect(context.modelUsageData.model.quotas[0].balance).toBe(80);
  });
  it('reports a failed batch once and preserves existing quota', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17) }));
    vi.mocked(api.queryModelUsage).mockResolvedValueOnce({
      success: false,
      error: 'Authentication failed',
    });
    await act(async () => {
      await context.refreshAllUsage();
    });
    expect(api.queryModelUsage).toHaveBeenCalledExactlyOnceWith('model');
    expect(context.modelUsageData.model.quotas[0].balance).toBe(17);
    expect(showToast).toHaveBeenCalledOnce();
    expect(context.isRefreshingUsage).toBe(false);
  });
  it('preserves quota for a name-only edit and passive view switching', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17) }));
    await edit({ name: 'Renamed' });
    act(() => context.setViewMode('usage'));
    act(() => context.setViewMode('config'));
    expect(context.userModels[0].name).toBe('Renamed');
    expect(context.modelUsageData.model.quotas[0].balance).toBe(17);
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });
  it('clears only the edited model when its endpoint changes', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17), other: balance(80) }));
    await edit({ baseUrl: 'https://api.example.test' });
    expect(context.modelUsageData.model).toBeUndefined();
    expect(context.modelUsageData.other.quotas[0].balance).toBe(80);
    expect(api.queryModelUsage).not.toHaveBeenCalled();
  });
  it('does not clear a valid cache when saving credentials fails', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17) }));
    vi.mocked(api.updateModel).mockRejectedValueOnce('write failed');
    const errorLog = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await edit({ apiKey: 'fake-new-key' });
    errorLog.mockRestore();
    expect(context.modelUsageData.model.quotas[0].balance).toBe(17);
    expect(context.userModels[0].apiKey).toBe('fake-old-key');
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledOnce();
  });
  it('deleted models cannot be resurrected by a late quota request', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17) }));
    let resolveOld!: (result: api.UsageResult) => void;
    vi.mocked(api.queryModelUsage).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.refreshSingleUsage('model');
    });
    await act(async () => {
      await context.handleCardDelete('model');
    });
    await act(async () => {
      resolveOld({ success: true, data: balance(80) });
      await pending;
    });
    expect(api.deleteModel).toHaveBeenCalledExactlyOnceWith('model');
    expect(context.userModels).toEqual([]);
    expect(context.modelUsageData.model).toBeUndefined();
    expect(context.refreshingUsageIds.size).toBe(0);
  });
  it('keeps the model and quota visible when durable deletion fails', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17) }));
    vi.mocked(api.deleteModel).mockRejectedValueOnce('Cannot save models');
    await act(async () => {
      await context.handleCardDelete('model');
    });
    expect(context.userModels).toHaveLength(1);
    expect(context.modelUsageData.model).toEqual(balance(17));
    expect(showToast).toHaveBeenCalledWith('error', 'Cannot save models');
  });
  it('reports mixed batch failures once, updates successes and keeps failed caches', async () => {
    vi.mocked(api.getModels).mockResolvedValue([
      original,
      { ...original, internalId: 'second', name: 'Second' },
      { ...original, internalId: 'third', name: 'Third' },
    ]);
    await open();
    act(() => context.setModelUsageData({ second: balance(17), third: balance(30) }));
    vi.mocked(api.queryModelUsage)
      .mockResolvedValueOnce({ success: true, data: balance(80) })
      .mockResolvedValueOnce({ success: false, error: 'Access denied' })
      .mockRejectedValueOnce('Network unavailable');
    await act(async () => {
      await context.refreshAllUsage();
    });
    expect(api.queryModelUsage).toHaveBeenCalledTimes(3);
    expect(context.modelUsageData.model.quotas[0].balance).toBe(80);
    expect(context.modelUsageData.second.quotas[0].balance).toBe(17);
    expect(context.modelUsageData.third.quotas[0].balance).toBe(30);
    expect(showToast).toHaveBeenCalledExactlyOnceWith(
      'error',
      'model.quota.refreshFailed Second, Third'
    );
    expect(context.isRefreshingUsage).toBe(false);
  });
  it('suppresses same-tick duplicate batches and ignores obsolete batch results', async () => {
    await open();
    let resolveOld!: (result: api.UsageResult) => void;
    vi.mocked(api.queryModelUsage).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.refreshAllUsage();
      void context.refreshAllUsage();
    });
    expect(api.queryModelUsage).toHaveBeenCalledTimes(1);
    await edit({ apiKey: 'fake-new-key' });
    await act(async () => {
      resolveOld({ success: true, data: balance(80) });
      await pending;
    });
    expect(context.modelUsageData.model).toBeUndefined();
    expect(context.isRefreshingUsage).toBe(false);
    expect(showToast).not.toHaveBeenCalled();
  });
  it('starts a fresh AK/SK query while the previous credential request is pending', async () => {
    await open();
    let resolveOld!: (result: api.UsageResult) => void;
    vi.mocked(api.queryModelUsage).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.refreshSingleUsage('model');
    });
    vi.mocked(api.queryModelUsage).mockResolvedValueOnce({ success: true, data: balance(80) });
    await act(async () => {
      await context.saveVolcAksk('model', 'fake-ak', 'fake-sk');
    });
    await act(async () => {
      resolveOld({ success: true, data: balance(17) });
      await pending;
    });
    expect(api.queryModelUsage).toHaveBeenCalledTimes(2);
    expect(context.modelUsageData.model.quotas[0].balance).toBe(80);
    expect(context.refreshingUsageIds.size).toBe(0);
  });
  it('blocks an overlapping single refresh during a batch and shows the row as busy', async () => {
    await open();
    let resolveBatch!: (result: api.UsageResult) => void;
    vi.mocked(api.queryModelUsage).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveBatch = resolve;
      })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.refreshAllUsage();
    });
    expect(context.refreshingUsageIds.has('model')).toBe(true);
    await act(async () => {
      await context.refreshSingleUsage('model');
    });
    expect(api.queryModelUsage).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveBatch({ success: false, error: 'Network unavailable' });
      await pending;
    });
    expect(context.refreshingUsageIds.size).toBe(0);
    expect(showToast).toHaveBeenCalledOnce();
  });
  it('a batch skips a model already refreshing individually and refreshes other models', async () => {
    vi.mocked(api.getModels).mockResolvedValue([original, { ...original, internalId: 'second' }]);
    await open();
    let resolveSingle!: (result: api.UsageResult) => void;
    vi.mocked(api.queryModelUsage).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveSingle = resolve;
      })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.refreshSingleUsage('model');
    });
    await act(async () => {
      await context.refreshAllUsage();
    });
    expect(vi.mocked(api.queryModelUsage).mock.calls).toEqual([['model'], ['second']]);
    expect(context.refreshingUsageIds.has('model')).toBe(true);
    expect(context.refreshingUsageIds.has('second')).toBe(false);
    await act(async () => {
      resolveSingle({ success: true, data: balance(80) });
      await pending;
    });
    expect(context.modelUsageData.model.quotas[0].balance).toBe(80);
    expect(context.modelUsageData.second.quotas[0].balance).toBe(17);
    expect(context.refreshingUsageIds.size).toBe(0);
  });
  it('shows an explicit rejected refresh once while preserving the cache', async () => {
    await open();
    act(() => context.setModelUsageData({ model: balance(17) }));
    vi.mocked(api.queryModelUsage).mockRejectedValueOnce('Network unavailable');
    await act(async () => {
      await context.refreshSingleUsage('model');
    });
    expect(context.modelUsageData.model.quotas[0].balance).toBe(17);
    expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'Network unavailable');
    expect(context.refreshingUsageIds.size).toBe(0);
  });
  it.each([
    ['https://ark.cn-beijing.volces.com/api/coding/v3', true],
    ['https://ark.cn-beijing.volces.com:443/api/coding/v1///', true],
    ['https://ark.cn-beijing.volces.com/api/v3', false],
    ['https://ark.ap-southeast.bytepluses.com/api/coding', false],
    ['https://ark.cn-beijing.volces.com/api/agent', false],
    ['https://ark.cn-beijing.volces.com:8443/api/coding', false],
    ['https://ark.cn-beijing.volces.com.example.test/api/coding', false],
  ])(
    'checks saved AK/SK only for supported CN Coding Plan routes: %s',
    async (baseUrl, supported) => {
      vi.mocked(api.getModels).mockResolvedValue([{ ...original, baseUrl }]);
      await open();
      expect(api.hasVolcAksk).toHaveBeenCalledTimes(supported ? 1 : 0);
      expect(api.queryModelUsage).not.toHaveBeenCalled();
      expect(api.saveVolcAksk).not.toHaveBeenCalled();
      expect(showToast).not.toHaveBeenCalled();
    }
  );
});
