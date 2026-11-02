import { create } from 'zustand';
import {
  currentMonitor,
  getCurrentWindow,
  PhysicalPosition,
  PhysicalSize,
  type Monitor,
} from '@tauri-apps/api/window';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { emitTo, listen } from '@tauri-apps/api/event';

export type UiScalePreference = 'auto' | number;
export const UI_SCALE_CHANGED_EVENT = 'echobird-ui-scale-changed';
const STORAGE_KEY = 'echobird-ui-scale';
let applied = false;
let queued: { preference: UiScalePreference; persist: boolean } | undefined;
let applying: Promise<void> | undefined;

export function automaticUiScale(monitor: Monitor | null): number {
  if (!monitor) return 1;
  // Native logical pixels are independent of WebView zoom (unlike devicePixelRatio).
  const width = monitor.workArea.size.width / monitor.scaleFactor;
  const height = monitor.workArea.size.height / monitor.scaleFactor;
  const fit = Math.min(width / 1400, height / 900);
  return fit >= 1 ? 1 : fit >= 0.9 ? 0.9 : 0.8;
}

export function scaledWindowBounds(
  scale: number,
  monitor: Monitor,
  current: { inner: PhysicalSize; outer: PhysicalSize; position: PhysicalPosition }
) {
  const area = monitor.workArea;
  const frameWidth = Math.max(0, current.outer.width - current.inner.width);
  const frameHeight = Math.max(0, current.outer.height - current.inner.height);
  const availableWidth = Math.max(1, area.size.width - frameWidth);
  const availableHeight = Math.max(1, area.size.height - frameHeight);
  const width = Math.min(Math.round(1400 * scale * monitor.scaleFactor), availableWidth);
  const height = Math.min(Math.round(900 * scale * monitor.scaleFactor), availableHeight);
  const x = current.position.x + (current.outer.width - width - frameWidth) / 2;
  const y = current.position.y + (current.outer.height - height - frameHeight) / 2;
  return {
    size: new PhysicalSize(width, height),
    minSize: new PhysicalSize(
      Math.min(Math.round(960 * monitor.scaleFactor), availableWidth),
      Math.min(Math.round(600 * monitor.scaleFactor), availableHeight)
    ),
    position: new PhysicalPosition(
      Math.round(Math.max(area.position.x, Math.min(x, area.position.x + availableWidth - width))),
      Math.round(Math.max(area.position.y, Math.min(y, area.position.y + availableHeight - height)))
    ),
  };
}

async function resizeWindow(scale: number): Promise<void> {
  const win = getCurrentWindow();
  const monitor = await currentMonitor();
  if (!monitor) return;
  if (await win.isMaximized()) await win.unmaximize();
  const [inner, outer, position] = await Promise.all([
    win.innerSize(),
    win.outerSize(),
    win.outerPosition(),
  ]);
  const bounds = scaledWindowBounds(scale, monitor, { inner, outer, position });
  await win.setMinSize(bounds.minSize);
  await win.setSize(bounds.size);
  await win.setPosition(bounds.position);
}

interface UiScaleStore {
  preference: UiScalePreference;
  requestedPreference: UiScalePreference | null;
  scale: number;
  pending: boolean;
  failed: boolean;
  setPreference: (preference: UiScalePreference, persist?: boolean) => Promise<void>;
}

export const useUiScaleStore = create<UiScaleStore>((set, get) => ({
  preference: 'auto',
  requestedPreference: null,
  scale: 1,
  pending: false,
  failed: false,
  setPreference: (preference, persist = true) => {
    if (preference !== 'auto') {
      preference = Math.max(70, Math.min(150, Math.round(preference / 10) * 10));
      const state = get();
      // Pointer movement within one step must not resize/recenter the window again.
      if (
        persist &&
        applied &&
        !state.failed &&
        (state.requestedPreference ?? state.preference) === preference
      ) {
        return applying ?? Promise.resolve();
      }
    }
    queued = { preference, persist };
    set({ requestedPreference: preference, pending: true, failed: false });
    // Coalesce drag events while native calls are in flight; always apply the last value.
    applying ??= (async () => {
      while (queued) {
        const request = queued;
        queued = undefined;
        try {
          const scale =
            request.preference === 'auto'
              ? automaticUiScale(await currentMonitor())
              : request.preference / 100;
          if (!applied || scale !== get().scale) {
            await getCurrentWebview().setZoom(scale);
            applied = true;
            if (getCurrentWindow().label === 'main') {
              await emitTo('voice-island', UI_SCALE_CHANGED_EVENT, scale).catch(() => {});
            }
          }
          set({ preference: request.preference, scale, failed: false });
          if (request.persist && !queued) {
            // Startup restores the saved window as-is; explicit changes resize it once.
            await resizeWindow(scale);
            if (!queued) localStorage.setItem(STORAGE_KEY, String(request.preference));
          }
        } catch (error) {
          console.error('[UiScale] Could not apply or save interface scale:', error);
          set({ failed: true });
        }
      }
      set({ requestedPreference: null, pending: false });
    })().finally(() => {
      applying = undefined;
    });
    return applying;
  },
}));

export async function initializeUiScale(): Promise<() => void> {
  const win = getCurrentWindow();
  let synced = false;
  const sync =
    win.label === 'voice-island'
      ? await listen<number>(UI_SCALE_CHANGED_EVENT, ({ payload }) => {
          if (Number.isFinite(payload) && payload >= 0.7 && payload <= 1.5) {
            synced = true;
            void useUiScaleStore.getState().setPreference(payload * 100, false);
          }
        }).catch(() => undefined)
      : undefined;
  let preference: UiScalePreference = 'auto';
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    const percent = Number(saved);
    if (saved !== null && Number.isInteger(percent) && percent >= 70 && percent <= 150)
      preference = percent;
  } catch {
    // Unavailable storage must not prevent the first window from opening.
  }
  if (!synced) await useUiScaleStore.getState().setPreference(preference, false);
  else await applying;

  let timer: ReturnType<typeof setTimeout> | undefined;
  const refresh = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const state = useUiScaleStore.getState();
      if ((state.requestedPreference ?? state.preference) === 'auto' && !state.pending) {
        void useUiScaleStore.getState().setPreference('auto', false);
      }
    }, 250);
  };
  const listeners = await Promise.allSettled([win.onMoved(refresh), win.onScaleChanged(refresh)]);
  return () => {
    sync?.();
    clearTimeout(timer);
    for (const listener of listeners) {
      if (listener.status === 'fulfilled') listener.value();
    }
  };
}
