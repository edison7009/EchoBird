import { beforeEach, expect, it, vi } from 'vitest';
import * as models from './models';
import * as router from './smartRouter';
import { createCustomRouterGroup, ROUTER_GROUP_LIMIT } from './customRouterGroup';
vi.mock('./models', () => ({ addModel: vi.fn(), deleteModel: vi.fn() }));
vi.mock('./smartRouter', () => ({
  listSmartRouters: vi.fn(),
  getSmartRouterAvailablePort: vi.fn(),
  saveSmartRouter: vi.fn(),
  setSmartRouterCandidates: vi.fn(),
  deleteSmartRouter: vi.fn(),
}));
const group = {
  id: 'smart-router-new',
  name: 'New',
  candidateIds: ['saved', 'created'],
} as router.SmartRouterConfig;
const input = {
  name: 'Fixture',
  baseUrl: 'https://example.invalid/v1',
  apiKey: 'fixture-key',
  modelId: 'fixture',
  scope: 'smartRouter' as const,
};
const existing = { ...input, internalId: 'saved' };
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(router.listSmartRouters).mockResolvedValue([
    { id: 'smart-router' } as router.SmartRouterConfig,
  ]);
  vi.mocked(router.getSmartRouterAvailablePort).mockResolvedValue(54321);
  vi.mocked(router.saveSmartRouter).mockResolvedValue(group);
  vi.mocked(router.setSmartRouterCandidates).mockResolvedValue(group);
  vi.mocked(router.deleteSmartRouter).mockResolvedValue();
  vi.mocked(models.addModel).mockResolvedValue({ ...input, internalId: 'created' });
  vi.mocked(models.deleteModel).mockResolvedValue(true);
});
it('creates a distinct group and installs the full selection without mutating an existing group', async () => {
  await createCustomRouterGroup(
    [{ existing, input }, { input }],
    'Router',
    () => true,
    'Claude · Custom switch'
  );
  expect(router.saveSmartRouter).toHaveBeenCalledWith(
    expect.objectContaining({ name: 'Claude · Custom switch', modelId: 'auto', port: 54321 })
  );
  expect(router.setSmartRouterCandidates).toHaveBeenCalledExactlyOnceWith(
    ['saved', 'created'],
    'smart-router-new'
  );
  expect(models.addModel).toHaveBeenCalledTimes(1);
  expect(router.deleteSmartRouter).not.toHaveBeenCalled();
});
it('rejects seven groups before any model, group or port mutation', async () => {
  vi.mocked(router.listSmartRouters).mockResolvedValue(Array.from({ length: 7 }, () => group));
  await expect(createCustomRouterGroup([{ input }], 'Router', () => true)).rejects.toThrow(
    ROUTER_GROUP_LIMIT
  );
  expect(models.addModel).not.toHaveBeenCalled();
  expect(router.saveSmartRouter).not.toHaveBeenCalled();
  expect(router.getSmartRouterAvailablePort).not.toHaveBeenCalled();
});
it('rechecks the cap before publication and removes only its own new models', async () => {
  vi.mocked(router.listSmartRouters)
    .mockResolvedValueOnce([group])
    .mockResolvedValueOnce(Array.from({ length: 7 }, () => group));
  await expect(
    createCustomRouterGroup([{ existing, input }, { input }], 'Router', () => true)
  ).rejects.toThrow(ROUTER_GROUP_LIMIT);
  expect(models.deleteModel).toHaveBeenCalledExactlyOnceWith('created');
  expect(router.saveSmartRouter).not.toHaveBeenCalled();
});
it('rolls back a rejected new group before deleting owned models, preserving existing models', async () => {
  vi.mocked(router.setSmartRouterCandidates).mockRejectedValue(new Error('fixture rejected'));
  await expect(
    createCustomRouterGroup([{ existing, input }, { input }], 'Router', () => true)
  ).rejects.toThrow('fixture rejected');
  expect(router.deleteSmartRouter).toHaveBeenCalledWith(group.id);
  expect(models.deleteModel).toHaveBeenCalledExactlyOnceWith('created');
  expect(vi.mocked(router.deleteSmartRouter).mock.invocationCallOrder[0]).toBeLessThan(
    vi.mocked(models.deleteModel).mock.invocationCallOrder[0]
  );
});
it('cleans up a model created after navigation without publishing a group', async () => {
  let current = true;
  vi.mocked(models.addModel).mockImplementation(async () => {
    current = false;
    return { ...input, internalId: 'created' };
  });
  await expect(createCustomRouterGroup([{ input }], 'Router', () => current)).rejects.toThrow(
    'CUSTOM_SWITCH_CANCELLED'
  );
  expect(models.deleteModel).toHaveBeenCalledWith('created');
  expect(router.saveSmartRouter).not.toHaveBeenCalled();
});
it('preserves referenced models when group cleanup fails', async () => {
  vi.mocked(router.setSmartRouterCandidates).mockRejectedValue(new Error('fixture rejected'));
  vi.mocked(router.deleteSmartRouter).mockRejectedValue(new Error('fixture cleanup failed'));
  await expect(createCustomRouterGroup([{ input }], 'Router', () => true)).rejects.toThrow(
    'fixture cleanup failed'
  );
  expect(models.deleteModel).not.toHaveBeenCalled();
});

it('attempts cleanup of every owned model if one deletion fails', async () => {
  vi.mocked(models.addModel)
    .mockResolvedValueOnce({ ...input, internalId: 'created-one' })
    .mockResolvedValueOnce({ ...input, internalId: 'created-two' });
  vi.mocked(router.setSmartRouterCandidates).mockRejectedValue(new Error('fixture rejected'));
  vi.mocked(models.deleteModel)
    .mockRejectedValueOnce(new Error('fixture cleanup failed'))
    .mockResolvedValueOnce(true);
  await expect(
    createCustomRouterGroup([{ input }, { input }], 'Router', () => true)
  ).rejects.toThrow('fixture cleanup failed');
  expect(vi.mocked(models.deleteModel).mock.calls.map(([id]) => id)).toEqual([
    'created-two',
    'created-one',
  ]);
});

it('rolls back owned models when the backend rejects a concurrent seventh-group creation', async () => {
  vi.mocked(router.saveSmartRouter).mockRejectedValue(new Error(ROUTER_GROUP_LIMIT));
  await expect(
    createCustomRouterGroup([{ existing, input }, { input }], 'Router', () => true)
  ).rejects.toThrow(ROUTER_GROUP_LIMIT);
  expect(models.deleteModel).toHaveBeenCalledExactlyOnceWith('created');
  expect(router.deleteSmartRouter).not.toHaveBeenCalled();
  expect(router.setSmartRouterCandidates).not.toHaveBeenCalled();
});
