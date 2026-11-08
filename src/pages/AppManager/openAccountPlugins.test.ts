import { beforeEach, expect, it, vi } from 'vitest';
import { useNavigationStore } from '../../stores/navigationStore';
const api = vi.hoisted(() => ({ getSettings: vi.fn(), saveSettings: vi.fn() }));
vi.mock('../../api/tauri', () => api);
import { openAccountPlugins } from './openAccountPlugins';
beforeEach(() => {
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
  vi.clearAllMocks();
});
it('opens Apps plugins directly without reading or writing the retired preview setting', async () => {
  await openAccountPlugins();
  expect(useNavigationStore.getState()).toMatchObject({
    developerMode: false,
    activePage: 'apps',
    appsTab: 'plugins',
    accountPluginsRequest: expect.any(Number),
  });
  expect(api.getSettings).not.toHaveBeenCalled();
  expect(api.saveSettings).not.toHaveBeenCalled();
  useNavigationStore.getState().goBack();
  expect(useNavigationStore.getState().appsTab).toBe('desktop');
});
