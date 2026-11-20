import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useNavigationStore } from '../../stores/navigationStore';
import { FreeModelsProvider, useFreeModels } from './provider';

const { showToast } = vi.hoisted(() => ({ showToast: vi.fn() }));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../api/tauri', () => ({
  listSmartRouters: vi.fn(),
  getSmartRouterCandidates: vi.fn(),
  getCachedFreeModelDirectory: vi.fn(),
  addModel: vi.fn(),
  deleteModel: vi.fn(),
  saveSmartRouter: vi.fn(),
  deleteSmartRouter: vi.fn(),
  setSmartRouterCandidateEnabled: vi.fn(),
  setSmartRouterCandidates: vi.fn(),
  replaceSmartRouterCandidate: vi.fn(),
  removeSmartRouterCandidate: vi.fn(),
  updateModel: vi.fn(),
}));

let renderer: ReactTestRenderer;
let context: ReturnType<typeof useFreeModels>;
let groups: api.SmartRouterConfig[];
const secondId = 'smart-router-second-fixture';
const newId = 'smart-router-new-fixture';
const candidate = {
  internalId: 'shared-model',
  name: 'Fixture',
  modelId: 'upstream',
  baseUrl: 'https://fixture.test/v1',
  apiKey: '',
};
const settings: api.SmartRouterSettings = {
  name: 'New',
  modelId: 'auto',
  port: 60001,
  apiKey: 'fixture-key',
};

function Harness() {
  const value = useFreeModels();
  useLayoutEffect(() => {
    context = value;
  });
  return null;
}
async function open() {
  await act(async () => {
    renderer = create(
      <FreeModelsProvider>
        <Harness />
      </FreeModelsProvider>
    );
  });
}
async function reenter() {
  await act(async () => useNavigationStore.setState({ activePage: 'models' }));
  await act(async () => useNavigationStore.setState({ activePage: 'freeModels' }));
}

beforeEach(() => {
  groups = ['smart-router', secondId].map((id, index) => ({
    id,
    name: `Group ${index + 1}`,
    masterEnabled: true,
    groupEnabled: true,
    enabled: true,
    candidateIds: [candidate.internalId],
    disabledCandidateIds: [],
    usableCandidateCount: 1,
    baseUrl: `http://127.0.0.1:${53683 + index}/v1`,
    port: 53683 + index,
    modelId: 'auto',
    running: true,
  }));
  useNavigationStore.setState({ activePage: 'freeModels' });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() });
  vi.spyOn(console, 'error').mockImplementation(() => undefined);
  vi.mocked(api.listSmartRouters).mockImplementation(async () =>
    groups.map((group) => ({ ...group }))
  );
  vi.mocked(api.getSmartRouterCandidates).mockResolvedValue([candidate]);
  vi.mocked(api.getCachedFreeModelDirectory).mockResolvedValue(null);
  vi.mocked(api.saveSmartRouter).mockImplementation(async () => {
    const saved = { ...groups[0], id: newId, name: settings.name, port: settings.port };
    groups.push(saved);
    return saved;
  });
  vi.mocked(api.deleteSmartRouter).mockImplementation(async (id) => {
    groups = groups.filter((group) => group.id !== id);
  });
  vi.mocked(api.setSmartRouterCandidateEnabled).mockImplementation(async (_id, _enabled, id) => {
    const group = groups.find((entry) => entry.id === (id ?? 'smart-router'));
    if (!group) throw new Error('Fixture group no longer exists');
    return { ...group, disabledCandidateIds: [candidate.internalId] };
  });
});

afterEach(() => {
  act(() => renderer?.unmount());
  vi.restoreAllMocks();
  vi.resetAllMocks();
  vi.unstubAllGlobals();
});

it('keeps a completed creation successful when its subsequent read fails and preserves the visible routing scope', async () => {
  await open();
  const cachedModels = context.customModels;
  vi.mocked(api.getSmartRouterCandidates).mockRejectedValueOnce(new Error('Fixture read failed'));
  await act(async () => context.saveRouter(settings));
  expect(api.saveSmartRouter).toHaveBeenCalledOnce();
  expect(context.activeRouterId).toBe('smart-router');
  expect(context.customModels).toBe(cachedModels);
  expect(showToast).toHaveBeenCalledWith('error', 'error.requestFailed');
  await act(async () => context.setCandidateEnabled(candidate.internalId, false));
  expect(api.setSmartRouterCandidateEnabled).toHaveBeenLastCalledWith(candidate.internalId, false);

  await reenter();
  expect(context.routers.map((group) => group.id)).toEqual(['smart-router', secondId, newId]);
  await act(async () => context.selectRouter(newId));
  expect(context.activeRouterId).toBe(newId);
  expect(api.getSmartRouterCandidates).toHaveBeenLastCalledWith(newId);
  expect(api.saveSmartRouter).toHaveBeenCalledOnce();
  expect(api.deleteSmartRouter).not.toHaveBeenCalled();
  expect(api.setSmartRouterCandidateEnabled).toHaveBeenCalledOnce();
});

it('keeps a completed deletion successful when the fallback read fails without sending old-row toggles to the fallback', async () => {
  await open();
  const cachedModels = context.customModels;
  vi.mocked(api.getSmartRouterCandidates).mockRejectedValueOnce(new Error('Fixture read failed'));
  await act(async () => context.deleteRouter('smart-router'));
  expect(api.deleteSmartRouter).toHaveBeenCalledOnce();
  expect(context.activeRouterId).toBe('smart-router');
  expect(context.customModels).toBe(cachedModels);
  await act(async () => context.setCandidateEnabled(candidate.internalId, false));
  expect(api.setSmartRouterCandidateEnabled).toHaveBeenLastCalledWith(candidate.internalId, false);
  expect([...context.pausedIds]).toEqual([]);

  await reenter();
  expect(context.activeRouterId).toBe(secondId);
  expect(context.routers.map((group) => group.id)).toEqual([secondId]);
  expect(api.setSmartRouterCandidateEnabled).toHaveBeenCalledOnce();
  expect(api.deleteSmartRouter).toHaveBeenCalledOnce();
  expect(api.saveSmartRouter).not.toHaveBeenCalled();
});

it.each(['toggle', 'add', 'copy', 'model', 'replace', 'remove', 'reorder'] as const)(
  'does not apply a queued %s from an old panel after a successful page-entry fallback',
  async (action) => {
    await open();
    groups = groups.slice(1);
    let finish!: (models: (typeof candidate)[]) => void;
    vi.mocked(api.getSmartRouterCandidates).mockImplementationOnce(
      () => new Promise((resolve) => (finish = resolve))
    );
    await reenter();
    let operation!: Promise<unknown>;
    act(() => {
      operation =
        action === 'toggle'
          ? context.setCandidateEnabled(candidate.internalId, false)
          : action === 'add' || action === 'copy'
            ? context.addSelectedModel(candidate, { copy: action === 'copy' })
            : action === 'model'
              ? context.changeSelectedModel(candidate.internalId, 'other')
              : action === 'replace'
                ? context.replaceSelectedModel(candidate.internalId, {
                    ...candidate,
                    internalId: 'replacement',
                  })
                : action === 'remove'
                  ? context.removeSelectedModel(candidate.internalId)
                  : context.reorderSelectedModel(candidate.internalId, 'other-model');
      void operation.catch(() => undefined);
    });
    await act(async () => {
      finish([candidate]);
      await operation.catch(() => undefined);
    });
    expect(context.activeRouterId).toBe(secondId);
    expect(api.setSmartRouterCandidateEnabled).not.toHaveBeenCalled();
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect(api.replaceSmartRouterCandidate).not.toHaveBeenCalled();
    expect(api.addModel).not.toHaveBeenCalled();
    expect(api.removeSmartRouterCandidate).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(api.saveSmartRouter).not.toHaveBeenCalled();
    expect(api.deleteSmartRouter).not.toHaveBeenCalled();
  }
);

it('attaches a copied model to the original group before a queued group switch', async () => {
  await open();
  const copy: typeof candidate & { scope: 'smartRouter' } = {
    ...candidate,
    internalId: 'copied-model',
    scope: 'smartRouter',
  };
  let finishCopy!: (model: typeof copy) => void;
  vi.mocked(api.addModel).mockImplementationOnce(
    () => new Promise((resolve) => (finishCopy = resolve))
  );
  vi.mocked(api.setSmartRouterCandidates).mockImplementationOnce(async (candidateIds) => ({
    ...groups[0],
    candidateIds,
  }));
  let addition!: Promise<void>;
  await act(async () => {
    addition = context.addSelectedModel(candidate, { copy: true });
  });
  expect(api.addModel).toHaveBeenCalledExactlyOnceWith({
    name: candidate.name,
    baseUrl: candidate.baseUrl,
    apiKey: candidate.apiKey,
    modelId: candidate.modelId,
    scope: 'smartRouter',
  });
  let switchGroup!: Promise<void>;
  await act(async () => {
    switchGroup = context.selectRouter(secondId);
  });
  expect(context.activeRouterId).toBe('smart-router');
  expect(api.getSmartRouterCandidates).not.toHaveBeenCalledWith(secondId);
  await act(async () => {
    finishCopy(copy);
    await Promise.all([addition, switchGroup]);
  });
  expect(api.setSmartRouterCandidates).toHaveBeenCalledExactlyOnceWith([
    candidate.internalId,
    copy.internalId,
  ]);
  expect(api.deleteModel).not.toHaveBeenCalled();
  expect(context.activeRouterId).toBe(secondId);
  expect([...context.selectedIds]).toEqual([candidate.internalId]);
});

it('deletes a new copy when attaching it fails and leaves the original group intact', async () => {
  await open();
  const copy = { ...candidate, internalId: 'copied-model', scope: 'smartRouter' as const };
  const rejected = new Error('Fixture candidate write failed');
  vi.mocked(api.addModel).mockResolvedValueOnce(copy);
  vi.mocked(api.setSmartRouterCandidates).mockRejectedValueOnce(rejected);
  vi.mocked(api.deleteModel).mockResolvedValueOnce(true);
  await act(async () => {
    await expect(context.addSelectedModel(candidate, { copy: true })).rejects.toBe(rejected);
  });
  expect(api.deleteModel).toHaveBeenCalledExactlyOnceWith(copy.internalId);
  expect([...context.selectedIds]).toEqual([candidate.internalId]);
});

it('replaces a paused row via the same-group command and reflects the returned pause', async () => {
  const replacement = { ...candidate, internalId: 'replacement', modelId: 'next-model' };
  groups[1].disabledCandidateIds = [candidate.internalId];
  await open();
  await act(async () => context.selectRouter(secondId));
  expect([...context.pausedIds]).toEqual([candidate.internalId]);
  vi.mocked(api.replaceSmartRouterCandidate).mockResolvedValueOnce({
    ...groups[1],
    candidateIds: [replacement.internalId],
    disabledCandidateIds: [replacement.internalId],
  });
  await act(async () => context.replaceSelectedModel(candidate.internalId, replacement));
  expect(api.replaceSmartRouterCandidate).toHaveBeenCalledExactlyOnceWith(
    candidate.internalId,
    replacement.internalId,
    secondId
  );
  expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
  expect([...context.pausedIds]).toEqual([replacement.internalId]);
  expect(context.customModels.map((model) => model.id)).toEqual([replacement.internalId]);
});

it.each(['save', 'delete'] as const)(
  'still reports a failed %s write without treating it as committed',
  async (action) => {
    await open();
    const reads = vi.mocked(api.listSmartRouters).mock.calls.length;
    const failure = new Error('Fixture write failed');
    if (action === 'save') vi.mocked(api.saveSmartRouter).mockRejectedValueOnce(failure);
    else vi.mocked(api.deleteSmartRouter).mockRejectedValueOnce(failure);
    await act(async () => {
      const operation =
        action === 'save' ? context.saveRouter(settings) : context.deleteRouter('smart-router');
      await expect(operation).rejects.toBe(failure);
    });
    expect(context.activeRouterId).toBe('smart-router');
    expect(api.listSmartRouters).toHaveBeenCalledTimes(reads);
    expect(showToast).not.toHaveBeenCalled();
  }
);

it.each(['add', 'reorder'] as const)(
  'requires a fresh read before a full-list %s after a failed page refresh and preserves external additions',
  async (action) => {
    const secondModel = { ...candidate, internalId: 'second-model' };
    const externalModel = { ...candidate, internalId: 'external-model' };
    groups[0].candidateIds.push(secondModel.internalId);
    vi.mocked(api.getSmartRouterCandidates).mockResolvedValue([candidate, secondModel]);
    await open();
    groups[0].candidateIds.push(externalModel.internalId);
    vi.mocked(api.getSmartRouterCandidates).mockRejectedValueOnce(new Error('Fixture read failed'));
    await reenter();
    vi.mocked(api.getSmartRouterCandidates).mockRejectedValueOnce(
      new Error('Fixture retry failed')
    );
    const edit = () =>
      action === 'add'
        ? context.addSelectedModel({ ...candidate, internalId: 'added-model' })
        : context.reorderSelectedModel(candidate.internalId, secondModel.internalId);
    await act(async () => {
      await edit().catch(() => undefined);
    });
    expect(api.setSmartRouterCandidates).not.toHaveBeenCalled();
    expect([...context.selectedIds]).toEqual([candidate.internalId, secondModel.internalId]);

    vi.mocked(api.getSmartRouterCandidates).mockResolvedValue([
      candidate,
      secondModel,
      externalModel,
    ]);
    vi.mocked(api.setSmartRouterCandidates).mockImplementation(async (candidateIds) => ({
      ...groups[0],
      candidateIds,
    }));
    await act(async () => edit());
    expect(api.setSmartRouterCandidates).toHaveBeenCalledOnce();
    expect(api.setSmartRouterCandidates).toHaveBeenLastCalledWith(
      action === 'add'
        ? [candidate.internalId, secondModel.internalId, externalModel.internalId, 'added-model']
        : [secondModel.internalId, candidate.internalId, externalModel.internalId]
    );
  }
);

it('retains the previous group and its write scope after a failed explicit selection, then allows retry', async () => {
  await open();
  vi.mocked(api.getSmartRouterCandidates).mockRejectedValueOnce(new Error('Fixture read failed'));
  await act(async () => context.selectRouter(secondId));
  expect(context.activeRouterId).toBe('smart-router');
  await act(async () => context.setCandidateEnabled(candidate.internalId, false));
  expect(api.setSmartRouterCandidateEnabled).toHaveBeenLastCalledWith(candidate.internalId, false);
  await act(async () => context.selectRouter(secondId));
  expect(context.activeRouterId).toBe(secondId);
  await act(async () => context.setCandidateEnabled(candidate.internalId, false));
  expect(api.setSmartRouterCandidateEnabled).toHaveBeenLastCalledWith(
    candidate.internalId,
    false,
    secondId
  );
});
