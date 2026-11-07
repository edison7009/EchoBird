import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/smartRouter';
import type { RouteModelNode } from './provider';
import { useRouteResponseTests } from './useRouteResponseTests';
import { finishConnectionTestBatch } from '../../api/models';

vi.mock('../../api/models', () => ({
  finishConnectionTestBatch: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../../api/smartRouter', () => ({
  testSmartRouterCandidate: vi.fn(),
  cancelSmartRouterProbe: vi.fn().mockResolvedValue(undefined),
}));

describe('explicit Smart Router response tests', () => {
  let renderer: ReactTestRenderer;
  let hook: ReturnType<typeof useRouteResponseTests>;
  let models: RouteModelNode[];
  let active: boolean;
  let routerId: string;
  let replies: ((result: api.SmartRouterProbeResult) => void)[];
  function Harness() {
    hook = useRouteResponseTests(models, active, routerId);
    return null;
  }
  async function render() {
    await act(async () => {
      if (renderer) renderer.update(<Harness />);
      else renderer = create(<Harness />);
    });
  }
  async function reply(index: number, result: api.SmartRouterProbeResult) {
    await act(async () => replies[index](result));
  }
  beforeEach(() => {
    renderer = undefined as unknown as ReactTestRenderer;
    active = true;
    routerId = 'first';
    models = ['api', 'account', 'keyless'].map((id) => ({
      id,
      internalId: id,
      provider: id,
      modelId: `${id}-model`,
      baseUrl: `https://${id}.example/v1`,
      ...(id === 'account'
        ? { accountSource: { provider: 'codex' as const, accountId: 'fixture-account' } }
        : {}),
    }));
    replies = [];
    vi.mocked(api.testSmartRouterCandidate).mockImplementation(
      () => new Promise((resolve) => replies.push(resolve))
    );
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
  });

  it('cancels probes and ignores late results when switching groups with the same candidates', async () => {
    models = [models[0]];
    await render();
    await act(async () => {
      void hook.test();
    });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledOnce();
    expect(vi.mocked(api.testSmartRouterCandidate).mock.calls[0][4]).toBe('first');
    routerId = 'second';
    await render();
    expect(api.cancelSmartRouterProbe).toHaveBeenCalledOnce();
    expect(hook.isTesting).toBe(false);
    await reply(0, { latencyMs: 42, error: null });
    expect(hook.results).toEqual({});
    await act(async () => {
      void hook.test();
    });
    expect(vi.mocked(api.testSmartRouterCandidate).mock.calls[1][4]).toBe('second');
    await reply(1, { latencyMs: 18, error: null });
    expect(hook.results.api.latencyMs).toBe(18);
    routerId = 'first';
    await render();
    expect(hook.results).toEqual({});
  });
  afterEach(async () => {
    if (renderer) await act(async () => renderer.unmount());
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it('discards completed results after leaving and returning to the same group', async () => {
    models = [models[0]];
    await render();
    await act(async () => {
      void hook.test();
    });
    await reply(0, { latencyMs: 42, error: null });
    expect(hook.results.api.latencyMs).toBe(42);
    routerId = 'second';
    await render();
    routerId = 'first';
    await render();
    expect(hook.results).toEqual({});
    expect(api.testSmartRouterCandidate).toHaveBeenCalledOnce();
    await act(async () => {
      void hook.test();
    });
    await reply(1, { latencyMs: 18, error: null });
    expect(hook.results.api.latencyMs).toBe(18);
  });

  it('does nothing on navigation; tests every selected binding with at most two requests', async () => {
    active = false;
    await render();
    await act(async () => hook.test());
    active = true;
    await render();
    expect(api.testSmartRouterCandidate).not.toHaveBeenCalled();
    await act(async () => {
      void hook.test();
    });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(2);
    expect(hook.pendingIds.size).toBe(3);
    await act(async () => hook.test());
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(2);
    await reply(0, { latencyMs: 0, error: null });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(3);
    expect(api.testSmartRouterCandidate).toHaveBeenNthCalledWith(
      2,
      'account',
      'account-model',
      expect.any(String),
      expect.any(String),
      'first'
    );
    await reply(1, { latencyMs: null, error: 'HTTP 401' });
    await reply(2, { latencyMs: null, error: 'timeout' });
    expect(hook.isTesting).toBe(false);
    expect(hook.results.api.latencyMs).toBe(0);
    expect(hook.results.account.error).toBe('HTTP 401');
    expect(hook.results.keyless.error).toBe('timeout');
    expect(api.cancelSmartRouterProbe).not.toHaveBeenCalled();
    const firstBatch = vi.mocked(api.testSmartRouterCandidate).mock.calls[0][3];
    expect(
      new Set(vi.mocked(api.testSmartRouterCandidate).mock.calls.map((call) => call[3]))
    ).toEqual(new Set([firstBatch]));
    expect(finishConnectionTestBatch).toHaveBeenCalledExactlyOnceWith(firstBatch);
    await act(async () => {
      void hook.test();
    });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(5);
    expect(vi.mocked(api.testSmartRouterCandidate).mock.calls[3][3]).not.toBe(firstBatch);
    expect(
      new Set(vi.mocked(api.testSmartRouterCandidate).mock.calls.map((call) => call[2])).size
    ).toBe(5);
  });

  it('keeps the default router lookup when no group ID was supplied', async () => {
    routerId = '';
    models = [models[2]];
    await render();
    await act(async () => {
      void hook.test();
    });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledExactlyOnceWith(
      'keyless',
      'keyless-model',
      expect.any(String),
      expect.any(String),
      undefined
    );
    await reply(0, { latencyMs: 12, error: null });
    expect(hook.results.keyless.latencyMs).toBe(12);
  });

  it('continues the same round while hidden and retains results when returning', async () => {
    await render();
    await act(async () => {
      void hook.test();
    });
    const batch = vi.mocked(api.testSmartRouterCandidate).mock.calls[0][3];
    active = false;
    await render();
    expect(api.cancelSmartRouterProbe).not.toHaveBeenCalled();
    expect(finishConnectionTestBatch).not.toHaveBeenCalled();
    expect(hook.isTesting).toBe(true);
    await act(async () => hook.test());
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(2);
    await reply(0, { latencyMs: 100, error: null });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(3);
    expect(vi.mocked(api.testSmartRouterCandidate).mock.calls[2][3]).toBe(batch);
    expect(hook.results.api.latencyMs).toBe(100);
    active = true;
    await render();
    expect(hook.pendingIds).toEqual(new Set(['account', 'keyless']));
    await act(async () => hook.test());
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(3);
    active = false;
    await render();
    await reply(1, { latencyMs: 200, error: null });
    await reply(2, { latencyMs: null, error: 'timeout' });
    expect(hook.isTesting).toBe(false);
    expect(finishConnectionTestBatch).toHaveBeenCalledExactlyOnceWith(batch);
    active = true;
    await render();
    expect(hook.results.api.latencyMs).toBe(100);
    expect(hook.results.account.latencyMs).toBe(200);
    expect(hook.results.keyless.error).toBe('timeout');
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(3);
    expect(api.cancelSmartRouterProbe).not.toHaveBeenCalled();
  });

  it('keeps the original round when API and account candidates are added and tests additions only in the next round', async () => {
    await render();
    await act(async () => {
      void hook.test();
    });
    const firstBatch = vi.mocked(api.testSmartRouterCandidate).mock.calls[0][3];
    await reply(0, { latencyMs: 123, error: null });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(3);
    models = [
      ...models,
      {
        id: 'added-api',
        internalId: 'added-api',
        provider: 'Added API',
        modelId: 'added-api-model',
        baseUrl: 'https://added.example/v1',
      },
      {
        id: 'added-account',
        internalId: 'added-account',
        provider: 'Added account',
        modelId: 'added-account-model',
        baseUrl: '',
        accountSource: { provider: 'codex', accountId: 'added-account-id' },
      },
    ];
    await render();
    expect(api.cancelSmartRouterProbe).not.toHaveBeenCalled();
    expect(finishConnectionTestBatch).not.toHaveBeenCalled();
    expect(hook.isTesting).toBe(true);
    expect(hook.pendingIds).toEqual(new Set(['account', 'keyless']));
    expect(hook.results.api.latencyMs).toBe(123);
    await reply(1, { latencyMs: 234, error: null });
    await reply(2, { latencyMs: 345, error: null });
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(3);
    expect(hook.results['added-api']).toBeUndefined();
    expect(hook.results['added-account']).toBeUndefined();
    expect(hook.isTesting).toBe(false);
    expect(finishConnectionTestBatch).toHaveBeenCalledExactlyOnceWith(firstBatch);
    await act(async () => {
      void hook.test();
    });
    for (let i = 3; i < 8; i++) await reply(i, { latencyMs: 100 + i, error: null });
    expect(
      vi
        .mocked(api.testSmartRouterCandidate)
        .mock.calls.slice(3)
        .map((call) => call[0])
    ).toEqual(['api', 'account', 'keyless', 'added-api', 'added-account']);
    expect(hook.results['added-api'].latencyMs).toBe(106);
    expect(hook.results['added-account'].latencyMs).toBe(107);
    expect(hook.isTesting).toBe(false);
  });

  it.each(['model', 'account', 'endpoint', 'remove'])(
    'cancels when the %s binding changes',
    async (change) => {
      await render();
      await act(async () => {
        void hook.test();
      });
      models = models.map((model) => ({ ...model }));
      if (change === 'model') models[1].modelId = 'new-model';
      if (change === 'account')
        models[1].accountSource = { provider: 'codex', accountId: 'new-account' };
      if (change === 'endpoint') models[0].baseUrl = 'https://changed.example/v1';
      if (change === 'remove') models = models.slice(1);
      await render();
      expect(api.cancelSmartRouterProbe).toHaveBeenCalledTimes(2);
      await reply(0, { latencyMs: 10, error: null });
      await reply(1, { latencyMs: 10, error: null });
      expect(hook.results).toEqual({});
      expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(2);
    }
  );

  it('preserves results and in-flight requests when only priority or pause changes', async () => {
    await render();
    await act(async () => {
      void hook.test();
    });
    await reply(0, { latencyMs: 321, error: null });
    models = [models[2], models[0], models[1]].map((model) => ({ ...model }));
    await render();
    expect(api.cancelSmartRouterProbe).not.toHaveBeenCalled();
    expect(hook.results.api.latencyMs).toBe(321);
    await reply(1, { latencyMs: 456, error: null });
    await reply(2, { latencyMs: 789, error: null });
    expect(hook.results.account.latencyMs).toBe(456);
    active = false;
    await render();
    expect(hook.results.api.latencyMs).toBe(321);
    models = models.map((model) => (model.id === 'api' ? { ...model, modelId: 'other' } : model));
    await render();
    expect(hook.results.api).toBeUndefined();
  });

  it('does not let a cancelled round clean up a newer round or replace its results', async () => {
    await render();
    await act(async () => {
      void hook.test();
    });
    const firstBatch = vi.mocked(api.testSmartRouterCandidate).mock.calls[0][3];
    models = models.map((model) => ({ ...model, baseUrl: `${model.baseUrl}/changed` }));
    await render();
    await act(async () => {
      void hook.test();
    });
    const secondBatch = vi.mocked(api.testSmartRouterCandidate).mock.calls[2][3];
    expect(secondBatch).not.toBe(firstBatch);
    await reply(0, { latencyMs: 1, error: null });
    await reply(1, { latencyMs: 1, error: null });
    expect(hook.results).toEqual({});
    expect(hook.pendingIds.size).toBe(3);
    expect(finishConnectionTestBatch).toHaveBeenCalledExactlyOnceWith(firstBatch);
    await reply(2, { latencyMs: 200, error: null });
    await reply(3, { latencyMs: 300, error: null });
    await reply(4, { latencyMs: 400, error: null });
    expect(hook.results.api.latencyMs).toBe(200);
    expect(hook.results.account.latencyMs).toBe(300);
    expect(hook.results.keyless.latencyMs).toBe(400);
    expect(finishConnectionTestBatch).toHaveBeenLastCalledWith(secondBatch);
    expect(finishConnectionTestBatch).toHaveBeenCalledTimes(2);
    expect(hook.isTesting).toBe(false);
  });

  it('reports IPC failures and cancels owned requests on unmount', async () => {
    vi.mocked(api.testSmartRouterCandidate).mockRejectedValueOnce(new Error('IPC unavailable'));
    await render();
    await act(async () => {
      void hook.test();
    });
    expect(hook.results.api.error).toBe('request_failed');
    expect(api.testSmartRouterCandidate).toHaveBeenCalledTimes(3);
    await act(async () => renderer.unmount());
    expect(api.cancelSmartRouterProbe).toHaveBeenCalledTimes(2);
    renderer = undefined as unknown as ReactTestRenderer;
  });
});
