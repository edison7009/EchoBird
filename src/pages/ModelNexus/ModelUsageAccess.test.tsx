import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { ModelNexusProvider } from './ModelNexus';
import { useModelNexus } from './context';

const { showToast } = vi.hoisted(() => {
  vi.stubGlobal('__APP_EDITION__', 'full');
  return { showToast: vi.fn() };
});
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../components', () => ({
  ModelCard: () => null,
  ModelCardSkeleton: () => null,
  ModelIdCombobox: () => null,
  getModelIcon: () => null,
}));
vi.mock('../FreeModels/provider', () => ({ useFreeModels: vi.fn() }));
vi.mock('../../api/tauri', () => ({
  getModels: vi.fn().mockResolvedValue([
    {
      internalId: 'glm',
      name: 'GLM',
      baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
      apiKey: 'test',
    },
  ]),
  getZhipuTeamAccess: vi.fn().mockResolvedValue(null),
  saveZhipuTeamAccess: vi.fn().mockResolvedValue(undefined),
  queryModelUsage: vi.fn().mockResolvedValue({ success: true, data: { quotas: [] } }),
}));

describe('Zhipu Team usage access', () => {
  let renderer: ReactTestRenderer;
  let context: ReturnType<typeof useModelNexus>;
  function Harness() {
    const state = useModelNexus();
    useLayoutEffect(() => {
      context = state;
    });
    return null;
  }
  async function open() {
    await act(async () => {
      renderer = create(
        <ModelNexusProvider>
          <Harness />
        </ModelNexusProvider>
      );
    });
    await act(async () => {
      await context.openZhipuTeamModal('glm');
    });
  }
  const saveButton = () =>
    renderer.root
      .findAllByType('button')
      .find((button) => button.children.includes('model.enterSave'))!;
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.clearAllMocks();
  });

  it('requires both team IDs, saves separately from the model key, and can return to a personal plan', async () => {
    await open();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    const inputs = renderer.root.findAllByType('input');
    act(() => inputs[0].props.onChange({ target: { value: ' org-test ' } }));
    expect(saveButton().props.disabled).toBe(true);
    act(() => inputs[1].props.onChange({ target: { value: ' project-test ' } }));
    await act(async () => {
      await saveButton().props.onClick();
    });
    expect(api.saveZhipuTeamAccess).toHaveBeenCalledWith('glm', {
      organizationId: 'org-test',
      projectId: 'project-test',
    });
    expect(api.queryModelUsage).toHaveBeenCalledExactlyOnceWith('glm');
    await act(async () => {
      await context.openZhipuTeamModal('glm');
    });
    await act(async () => {
      await saveButton().props.onClick();
    });
    expect(api.saveZhipuTeamAccess).toHaveBeenLastCalledWith('glm', null);
  });

  it('keeps entered values when saving fails and does not issue a quota query', async () => {
    await open();
    const inputs = renderer.root.findAllByType('input');
    act(() => {
      inputs[0].props.onChange({ target: { value: 'org-test' } });
      inputs[1].props.onChange({ target: { value: 'project-test' } });
    });
    vi.mocked(api.saveZhipuTeamAccess).mockRejectedValueOnce(new Error('write failed'));
    await act(async () => {
      await saveButton().props.onClick();
    });
    expect(renderer.root.findAllByType('input')[0].props.value).toBe('org-test');
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith('error', 'model.quota.accessFailed');
  });

  it('clears the previous plan quota after changing access, even if the new query fails', async () => {
    await open();
    act(() =>
      context.setModelUsageData({
        glm: { quotas: [{ period: 'weekly', percentage: 20, resetAt: 0 }] },
        other: { quotas: [{ balance: 12, percentage: 0, resetAt: 0 }] },
      })
    );
    vi.mocked(api.queryModelUsage).mockResolvedValueOnce({
      success: false,
      error: 'Access denied',
    });
    await act(async () => {
      await saveButton().props.onClick();
    });
    expect(context.modelUsageData.glm).toBeUndefined();
    expect(context.modelUsageData.other.quotas[0].balance).toBe(12);
  });

  it('ignores a late quota response from the previous access configuration', async () => {
    await open();
    let resolveOld!: (result: api.UsageResult) => void;
    vi.mocked(api.queryModelUsage).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      })
    );
    let pending!: Promise<void>;
    act(() => {
      pending = context.refreshSingleUsage('glm');
    });
    vi.mocked(api.queryModelUsage).mockResolvedValueOnce({
      success: true,
      data: {
        quotas: [{ period: 'weekly', percentage: 25, resetAt: 0 }],
      },
    });
    await act(async () => {
      await saveButton().props.onClick();
    });
    await act(async () => {
      resolveOld({
        success: true,
        data: { quotas: [{ period: 'weekly', percentage: 90, resetAt: 0 }] },
      });
      await pending;
    });
    expect(context.modelUsageData.glm.quotas[0].percentage).toBe(25);
    expect(context.refreshingUsageIds.has('glm')).toBe(false);
  });
});
