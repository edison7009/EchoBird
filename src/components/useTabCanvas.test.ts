import { afterEach, expect, it, vi } from 'vitest';
import { mountTabCanvas } from './useTabCanvas';
import { paintTabCanvas } from './tabCanvas';
vi.mock('./tabCanvas', () => ({ paintTabCanvas: vi.fn() }));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it.each([false, true])(
  'tracks selection, pauses hidden paint and releases resources (independent=%s)',
  (independent) => {
    let width = 120;
    let selectedX = 1;
    let hidden = false;
    let reduced = false;
    const tasks = new Map<number, FrameRequestCallback>();
    let serial = 0;
    const observers: { callback: () => void; disconnect: ReturnType<typeof vi.fn> }[] = [];
    const frame = () => {
      const entries = [...tasks];
      tasks.clear();
      entries.forEach(([, callback]) => callback(performance.now() + 300));
    };
    const canvas = {
      className: '',
      style: {},
      width: 300,
      height: 150,
      setAttribute: vi.fn(),
      remove: vi.fn(),
      getContext: () => ({ clearRect: vi.fn() }),
    };
    const colors = { className: '', remove: vi.fn() };
    const selected = {
      offsetWidth: 50,
      offsetHeight: 30,
      getBoundingClientRect: () => ({ left: selectedX, top: 1, width: 50, height: 30 }),
    };
    const style = {
      borderTopColor: 'rgb(80,80,80)',
      borderTopLeftRadius: '6px',
      backgroundColor: 'rgb(100,60,50)',
      color: 'rgb(255,200,180)',
      getPropertyValue: (key: string) => (key.startsWith('border-') ? '1px' : '0px'),
    };
    const group = {
      style: { borderWidth: '', padding: '' },
      dataset: {} as Record<string, string>,
      scrollLeft: 0,
      scrollTop: 0,
      prepend: vi.fn(),
      append: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      querySelector: () => selected,
      querySelectorAll: (selector: string) =>
        independent && selector.includes('aria-pressed')
          ? selectedX < 0
            ? []
            : [
                selected,
                {
                  ...selected,
                  getBoundingClientRect: () => ({ left: 60, top: 1, width: 50, height: 30 }),
                },
              ]
          : [selected],
      getBoundingClientRect: () => ({ left: 0, top: 0, width, height: 32 }),
    };
    const doc = {
      get hidden() {
        return hidden;
      },
      documentElement: {},
      createElement: (tag: string) => (tag === 'canvas' ? canvas : colors),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    };
    const motion = {
      get matches() {
        return reduced;
      },
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal('document', doc);
    vi.stubGlobal('getComputedStyle', () => style);
    vi.stubGlobal('matchMedia', () => motion);
    vi.stubGlobal('devicePixelRatio', 2);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      const id = ++serial;
      tasks.set(id, callback);
      return id;
    });
    vi.stubGlobal('cancelAnimationFrame', (id: number) => tasks.delete(id));
    class Observer {
      callback: () => void;
      disconnect = vi.fn();
      observe = vi.fn();
      constructor(callback: () => void) {
        this.callback = callback;
        observers.push(this);
      }
    }
    vi.stubGlobal('ResizeObserver', Observer);
    vi.stubGlobal('MutationObserver', Observer);
    const remembered = { current: null };
    const cleanup = mountTabCanvas(group as unknown as HTMLDivElement, remembered, independent);
    frame();
    expect(paintTabCanvas).toHaveBeenCalled();
    if (independent)
      expect(vi.mocked(paintTabCanvas).mock.calls[0][4]).toEqual([
        { x: 1, y: 1, width: 50, height: 30 },
        { x: 60, y: 1, width: 50, height: 30 },
      ]);
    else
      expect(vi.mocked(paintTabCanvas).mock.calls[0][4]).toMatchObject({
        x: 0,
        y: 0,
        width: 51,
        height: 32,
      });
    selectedX = 60;
    observers[1].callback();
    frame();
    const next = vi.mocked(paintTabCanvas).mock.calls[1][4];
    expect(Array.isArray(next) ? next[0].x : next.x).toBe(60);
    hidden = true;
    doc.addEventListener.mock.calls[0][1]();
    frame();
    const count = vi.mocked(paintTabCanvas).mock.calls.length;
    expect(tasks.size).toBe(0);
    hidden = false;
    reduced = true;
    doc.addEventListener.mock.calls[0][1]();
    frame();
    expect(vi.mocked(paintTabCanvas).mock.calls.length).toBe(count + 1);
    expect(tasks.size).toBe(0);
    reduced = false;
    width = 0;
    observers[0].callback();
    expect(tasks.size).toBe(0);
    width = 120;
    observers[0].callback();
    expect(tasks.size).toBe(1);
    if (independent) {
      selectedX = -1;
      observers[1].callback();
      expect(tasks.size).toBe(0);
      selectedX = 1;
      observers[1].callback();
      expect(tasks.size).toBe(1);
    }
    cleanup();
    expect(tasks.size).toBe(0);
    expect(observers.every((observer) => observer.disconnect.mock.calls.length === 1)).toBe(true);
    expect(canvas.remove).toHaveBeenCalledOnce();
    expect(group.style).toEqual({ borderWidth: '', padding: '' });
    expect(group.dataset.tabRenderer).toBeUndefined();
  }
);
