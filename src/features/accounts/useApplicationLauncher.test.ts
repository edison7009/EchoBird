import { expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useApplicationLauncher } from './useApplicationLauncher';

vi.mock('../../api/tauri', () => ({ launchSystemApplication: vi.fn() }));

it('launches a system application by identity and surfaces launch errors', async () => {
  const setIsLaunching = vi.fn();
  const setApplyError = vi.fn();
  const launch = useApplicationLauncher({
    selectedTool: 'system:one',
    isLaunching: false,
    detectedTools: [{ id: 'system:one', systemAppId: 'system:one' }],
    setIsLaunching,
    setApplyError,
  } as unknown as Parameters<typeof useApplicationLauncher>[0]);
  vi.mocked(api.launchSystemApplication).mockResolvedValueOnce(undefined);
  await launch();
  expect(api.launchSystemApplication).toHaveBeenCalledExactlyOnceWith('system:one');
  expect(setIsLaunching.mock.calls).toEqual([[true], [false]]);
  vi.mocked(api.launchSystemApplication).mockRejectedValueOnce(new Error('Removed'));
  await launch();
  expect(setApplyError).toHaveBeenLastCalledWith('Error: Removed');
  expect(setIsLaunching).toHaveBeenLastCalledWith(false);
});
