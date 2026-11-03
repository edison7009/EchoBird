import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PhysicalPosition, PhysicalSize, type Monitor } from '@tauri-apps/api/window';

const native = vi.hoisted(() => ({
  label: 'main',
  emit: vi.fn(),
  listen: vi.fn(),
  monitor: vi.fn(),
  zoom: vi.fn(),
  moved: vi.fn(),
  dpi: vi.fn(),
  isMaximized: vi.fn(),
  unmaximize: vi.fn(),
  innerSize: vi.fn(),
  outerSize: vi.fn(),
  outerPosition: vi.fn(),
  setSize: vi.fn(),
  setMinSize: vi.fn(),
  setPosition: vi.fn(),
}));
vi.mock('@tauri-apps/api/window', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/window')>()),
  currentMonitor: native.monitor,
  getCurrentWindow: () => ({ ...native, onMoved: native.moved, onScaleChanged: native.dpi }),
}));
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ setZoom: native.zoom }) }));
vi.mock('@tauri-apps/api/event', () => ({ emitTo: native.emit, listen: native.listen }));

const monitor = (width: number, height: number, scaleFactor = 1): Monitor => ({
  name: 'display',
  size: new PhysicalSize(width, height),
  position: new PhysicalPosition(0, 0),
  workArea: { size: new PhysicalSize(width, height), position: new PhysicalPosition(0, 0) },
  scaleFactor,
});
let module: typeof import('./uiScaleStore');
let storage: { getItem: ReturnType<typeof vi.fn>; setItem: ReturnType<typeof vi.fn> };

beforeEach(async () => {
  vi.resetModules();
  vi.resetAllMocks();
  vi.useFakeTimers();
  native.label = 'main';
  native.emit.mockResolvedValue(undefined);
  native.listen.mockResolvedValue(vi.fn());
  storage = { getItem: vi.fn().mockReturnValue(null), setItem: vi.fn() };
  vi.stubGlobal('localStorage', storage);
  native.monitor.mockResolvedValue(monitor(1920, 1040));
  native.zoom.mockResolvedValue(undefined);
  native.moved.mockResolvedValue(vi.fn());
  native.dpi.mockResolvedValue(vi.fn());
  native.isMaximized.mockResolvedValue(false);
  native.innerSize.mockResolvedValue(new PhysicalSize(1400, 900));
  native.outerSize.mockResolvedValue(new PhysicalSize(1400, 900));
  native.outerPosition.mockResolvedValue(new PhysicalPosition(100, 50));
  vi.spyOn(console, 'error').mockImplementation(() => {});
  module = await import('./uiScaleStore');
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('UI scale', () => {
  it('keeps the voice island out of the interface scale', async () => {
    native.monitor.mockResolvedValue(monitor(1366, 728));
    await module.initializeUiScale();
    await module.useUiScaleStore.getState().setPreference(90);
    expect(native.zoom).toHaveBeenLastCalledWith(0.9);
    expect(native.emit).not.toHaveBeenCalled();
    expect(native.listen).not.toHaveBeenCalled();
  });

  it.each([
    [1920, 1040, 1, 1],
    [1440, 860, 1, 0.9],
    [1366, 728, 1, 0.8],
    [1920, 1032, 1.5, 0.8],
    [2880, 1720, 2, 0.9],
    [3840, 2080, 2, 1],
    [1024, 568, 1, 0.8],
  ])('fits a %s×%s work area at %s DPI scale to %s', (width, height, dpi, expected) => {
    expect(module.automaticUiScale(monitor(width, height, dpi))).toBe(expected);
  });

  it('defaults a first launch to automatic without writing settings', async () => {
    native.monitor.mockResolvedValue(monitor(1920, 1032, 1.5));
    await module.initializeUiScale();
    expect(native.zoom).toHaveBeenCalledExactlyOnceWith(0.8);
    expect(module.useUiScaleStore.getState()).toMatchObject({ preference: 'auto', scale: 0.8 });
    expect(storage.setItem).not.toHaveBeenCalled();
    expect(native.setSize).not.toHaveBeenCalled();
  });

  it('restores a manual preference without consulting or overriding it for the display', async () => {
    storage.getItem.mockReturnValue('130');
    await module.initializeUiScale();
    native.moved.mock.calls[0][0]();
    native.dpi.mock.calls[0][0]();
    await vi.runAllTimersAsync();
    expect(native.zoom).toHaveBeenCalledExactlyOnceWith(1.3);
    expect(native.monitor).not.toHaveBeenCalled();
    expect(storage.setItem).not.toHaveBeenCalled();
  });

  it('applies and saves explicit changes, including returning to automatic', async () => {
    await module.initializeUiScale();
    await module.useUiScaleStore.getState().setPreference(90);
    expect(native.zoom).toHaveBeenLastCalledWith(0.9);
    expect(storage.setItem).toHaveBeenLastCalledWith('echobird-ui-scale', '90');
    expect(native.setSize).toHaveBeenLastCalledWith(new PhysicalSize(1260, 810));
    await module.useUiScaleStore.getState().setPreference('auto');
    expect(native.zoom).toHaveBeenLastCalledWith(1);
    expect(storage.setItem).toHaveBeenLastCalledWith('echobird-ui-scale', 'auto');
  });

  it('avoids repeated zoom or writes on same-display moves, and adapts after a display change', async () => {
    const cleanup = await module.initializeUiScale();
    const moved = native.moved.mock.calls[0][0];
    moved();
    moved();
    await vi.runAllTimersAsync();
    expect(native.zoom).toHaveBeenCalledTimes(1);
    native.monitor.mockResolvedValue(monitor(1366, 728));
    native.dpi.mock.calls[0][0]();
    await vi.runAllTimersAsync();
    expect(native.zoom).toHaveBeenLastCalledWith(0.8);
    expect(storage.setItem).not.toHaveBeenCalled();
    moved();
    cleanup();
    await vi.runAllTimersAsync();
    expect(native.monitor).toHaveBeenCalledTimes(3);
    expect(await native.moved.mock.results[0].value).toHaveBeenCalledOnce();
  });

  it('preserves the previous scale and preference on native failure, then allows retry', async () => {
    await module.initializeUiScale();
    native.zoom.mockRejectedValueOnce(new Error('zoom failed'));
    await module.useUiScaleStore.getState().setPreference(80);
    expect(module.useUiScaleStore.getState()).toMatchObject({
      preference: 'auto',
      scale: 1,
      pending: false,
      failed: true,
    });
    expect(storage.setItem).not.toHaveBeenCalled();
    await module.useUiScaleStore.getState().setPreference(80);
    expect(module.useUiScaleStore.getState()).toMatchObject({
      preference: 80,
      scale: 0.8,
      failed: false,
    });
  });

  it('coalesces continuous drag requests and applies the final value, even while pending', async () => {
    let finish!: () => void;
    native.zoom.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        finish = resolve;
      })
    );
    const pending = module.useUiScaleStore.getState().setPreference(90);
    expect(module.useUiScaleStore.getState().pending).toBe(true);
    void module.useUiScaleStore.getState().setPreference(120);
    void module.useUiScaleStore.getState().setPreference(150);
    expect(module.useUiScaleStore.getState().requestedPreference).toBe(150);
    finish();
    await pending;
    expect(native.zoom.mock.calls).toEqual([[0.9], [1.5]]);
    expect(native.setSize).toHaveBeenCalledExactlyOnceWith(new PhysicalSize(1920, 1040));
    expect(storage.setItem).toHaveBeenCalledExactlyOnceWith('echobird-ui-scale', '150');
    expect(module.useUiScaleStore.getState()).toMatchObject({
      preference: 150,
      requestedPreference: null,
      pending: false,
    });
  });

  it('falls back for invalid saved values and absent display information', async () => {
    storage.getItem.mockReturnValue('invalid');
    native.monitor.mockResolvedValue(null);
    await module.initializeUiScale();
    expect(native.zoom).toHaveBeenCalledExactlyOnceWith(1);
    expect(module.useUiScaleStore.getState().preference).toBe('auto');
  });

  it('opens if storage or monitor reads fail and exposes a failed save', async () => {
    storage.getItem.mockImplementation(() => {
      throw new Error('storage unavailable');
    });
    native.monitor.mockRejectedValueOnce(new Error('display unavailable'));
    await module.initializeUiScale();
    expect(module.useUiScaleStore.getState()).toMatchObject({ failed: true, pending: false });
    storage.setItem.mockImplementation(() => {
      throw new Error('storage unavailable');
    });
    await module.useUiScaleStore.getState().setPreference(90);
    expect(module.useUiScaleStore.getState()).toMatchObject({
      scale: 0.9,
      failed: true,
      pending: false,
    });
  });

  it.each([
    [70, 70],
    [77, 80],
    [103, 100],
    [125, 130],
    [150, 150],
  ])(
    'restores legacy %s percent as the fixed %s percent step without resizing again',
    async (percent, expected) => {
      storage.getItem.mockReturnValue(String(percent));
      await module.initializeUiScale();
      expect(native.zoom).toHaveBeenCalledExactlyOnceWith(expected / 100);
      expect(module.useUiScaleStore.getState().preference).toBe(expected);
      expect(native.setSize).not.toHaveBeenCalled();
      expect(storage.setItem).not.toHaveBeenCalled();
    }
  );

  it('does not repeatedly resize or save while moving within the same fixed step', async () => {
    await module.initializeUiScale();
    native.zoom.mockClear();
    for (const percent of [77, 78, 80, 81, 84]) {
      await module.useUiScaleStore.getState().setPreference(percent);
    }
    expect(native.zoom).toHaveBeenCalledExactlyOnceWith(0.8);
    expect(native.setSize).toHaveBeenCalledExactlyOnceWith(new PhysicalSize(1120, 720));
    expect(storage.setItem).toHaveBeenCalledExactlyOnceWith('echobird-ui-scale', '80');
    await module.useUiScaleStore.getState().setPreference(85);
    expect(native.zoom).toHaveBeenLastCalledWith(0.9);
    expect(native.setSize).toHaveBeenCalledTimes(2);
  });

  it.each(['69', '151', 'NaN', 'Infinity', '88.5'])(
    'rejects an invalid stored percentage %s',
    async (saved) => {
      storage.getItem.mockReturnValue(saved);
      await module.initializeUiScale();
      expect(module.useUiScaleStore.getState().preference).toBe('auto');
    }
  );

  it('restores a maximized window and uses absolute sizes so screen clamping never accumulates drift', async () => {
    native.isMaximized.mockResolvedValueOnce(true);
    await module.useUiScaleStore.getState().setPreference(150);
    expect(native.unmaximize).toHaveBeenCalledOnce();
    expect(native.setSize).toHaveBeenLastCalledWith(new PhysicalSize(1920, 1040));
    await module.useUiScaleStore.getState().setPreference(70);
    expect(native.setSize).toHaveBeenLastCalledWith(new PhysicalSize(980, 630));
    await module.useUiScaleStore.getState().setPreference(100);
    expect(native.setSize).toHaveBeenLastCalledWith(new PhysicalSize(1400, 900));
  });

  it('includes DPI, native decorations and negative monitor positions in its work-area clamp', () => {
    const display = monitor(1920, 1040, 1.5);
    display.workArea.position = new PhysicalPosition(-1920, 40);
    const bounds = module.scaledWindowBounds(1.5, display, {
      inner: new PhysicalSize(1400, 900),
      outer: new PhysicalSize(1416, 939),
      position: new PhysicalPosition(-1500, 200),
    });
    expect(bounds.size).toEqual(new PhysicalSize(1904, 1001));
    expect(bounds.position).toEqual(new PhysicalPosition(-1920, 40));
  });

  it('centers proportional changes when space is available', () => {
    const bounds = module.scaledWindowBounds(0.7, monitor(3840, 2160, 2), {
      inner: new PhysicalSize(2800, 1800),
      outer: new PhysicalSize(2800, 1800),
      position: new PhysicalPosition(500, 100),
    });
    expect(bounds.size).toEqual(new PhysicalSize(1960, 1260));
    expect(bounds.position).toEqual(new PhysicalPosition(920, 370));
  });

  it('reports native resize failures and does not save an incomplete change', async () => {
    native.setSize.mockRejectedValueOnce(new Error('window resize failed'));
    await module.useUiScaleStore.getState().setPreference(70);
    expect(module.useUiScaleStore.getState()).toMatchObject({
      failed: true,
      pending: false,
      scale: 0.7,
    });
    expect(storage.setItem).not.toHaveBeenCalled();
    await module.useUiScaleStore.getState().setPreference(70);
    expect(storage.setItem).toHaveBeenCalledExactlyOnceWith('echobird-ui-scale', '70');
  });
});
