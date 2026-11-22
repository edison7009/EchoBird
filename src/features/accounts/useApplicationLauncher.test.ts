import { expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import { useApplicationLauncher } from './useApplicationLauncher';

vi.mock('../../api/tauri', () => ({
  launchSystemApplication: vi.fn(),
  startTool: vi.fn(),
}));

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

it.each(['qoder', 'qodercn', 'claudescience'])(
  'honors the direct-launch option after applying a model for %s',
  async (selectedTool) => {
    vi.useFakeTimers();
    try {
      vi.mocked(api.startTool).mockClear();
      const applyModelConfig = vi.fn().mockResolvedValue(true);
      const setApplyError = vi.fn();
      const baseOptions = {
        selectedTool,
        isLaunching: false,
        qoderAccounts: { busy: false, selectedId: null },
        launchGenerationRef: { current: 0 },
        previewLaunchRef: { current: false },
        setIsLaunching: vi.fn(),
        setApplyError,
        detectedTools: [{ id: selectedTool, category: 'Desktop', noModelConfig: false }],
        toolModelConfig: { [selectedTool]: 'third-party' },
        agreedConfigPolicy: true,
        applyModelConfig,
      } as unknown as Omit<Parameters<typeof useApplicationLauncher>[0], 'launchAfterApply'>;

      await useApplicationLauncher({ ...baseOptions, launchAfterApply: false })();
      expect(applyModelConfig).toHaveBeenCalledExactlyOnceWith(selectedTool, 'third-party');
      expect(api.startTool).not.toHaveBeenCalled();
      expect(setApplyError).not.toHaveBeenCalled();

      await useApplicationLauncher({ ...baseOptions, launchAfterApply: true })();
      expect(applyModelConfig).toHaveBeenCalledTimes(2);
      expect(api.startTool).toHaveBeenCalledExactlyOnceWith(selectedTool, undefined, undefined);
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  }
);
