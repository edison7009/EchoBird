import { afterEach, expect, it, vi } from 'vitest';
import { mountTabCanvas } from './useTabCanvas';
import { paintTabCanvas } from './tabCanvas';
vi.mock('./tabCanvas', () => ({ paintTabCanvas: vi.fn() }));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it('tracks selection, pauses hidden paint and releases resources', () => {
  let width = 120;
  let selectedX = 60;
  let hidden = false;
  let reduced = false;
  let hasSelection = true;
  const tasks = new Map<number, FrameRequestCallback>();
  let serial = 0;
  const observers: Observer[] = [];
  const frame = (delay = 300) => {
    const entries = [...tasks];
    tasks.clear();
    entries.forEach(([, callback]) => callback(performance.now() + delay));
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
  let selected = {
    offsetWidth: 50,
    offsetHeight: 30,
    getBoundingClientRect: () => ({ left: selectedX, top: 1, width: 50, height: 30 }),
  };
  const style = {
    borderTopWidth: '1px',
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
    querySelector: () => (hasSelection ? selected : null),
    querySelectorAll: () => [selected],
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
    disconnect = vi.fn();
    observe = vi.fn();
    unobserve = vi.fn();
    constructor(private readonly update: (records: Pick<MutationRecord, 'type'>[]) => void) {
      observers.push(this);
    }
    callback(records: Pick<MutationRecord, 'type'>[] = []) {
      this.update(records);
    }
  }
  vi.stubGlobal('ResizeObserver', Observer);
  vi.stubGlobal('MutationObserver', Observer);
  const remembered = { current: null };
  const cleanup = mountTabCanvas(group as unknown as HTMLDivElement, remembered);
  frame(0);
  expect(paintTabCanvas).toHaveBeenCalledOnce();
  expect(tasks.size).toBe(0);
  expect(vi.mocked(paintTabCanvas).mock.calls[0][4]).toMatchObject({
    x: 60,
    y: 0,
    width: 50,
    height: 32,
  });
  selectedX = 1;
  observers[1].callback();
  frame(-1_000);
  expect(vi.mocked(paintTabCanvas).mock.calls.slice(-1)[0][4]).toMatchObject({ x: 60 });
  frame(0);
  const moving = vi.mocked(paintTabCanvas).mock.calls.slice(-1)[0][4];
  expect(moving?.x).toBeGreaterThan(0);
  frame();
  const next = vi.mocked(paintTabCanvas).mock.calls.slice(-1)[0][4];
  expect(next?.x).toBe(0);
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
  hasSelection = false;
  observers[1].callback();
  let last = vi.mocked(paintTabCanvas).mock.calls.slice(-1)[0];
  expect(last[4]).toBeNull();
  expect(last[5].border).toBe('rgb(80,80,80)');
  expect(tasks.size).toBe(0);
  style.borderTopColor = 'rgb(90,90,90)';
  observers[2].callback();
  last = vi.mocked(paintTabCanvas).mock.calls.slice(-1)[0];
  expect(last[4]).toBeNull();
  expect(last[5].border).toBe('rgb(90,90,90)');
  expect(tasks.size).toBe(0);
  hasSelection = true;
  observers[1].callback();
  expect(tasks.size).toBe(1);
  const previousTab = selected;
  selected = { ...previousTab };
  observers[1].callback([{ type: 'childList' }]);
  expect(observers[0].unobserve).toHaveBeenCalledWith(previousTab);
  expect(observers[0].observe).toHaveBeenCalledWith(selected);
  cleanup();
  expect(tasks.size).toBe(0);
  expect(observers.every((observer) => observer.disconnect.mock.calls.length === 1)).toBe(true);
  expect(canvas.remove).toHaveBeenCalledOnce();
  expect(group.style).toEqual({ borderWidth: '', padding: '' });
  expect(group.dataset.tabRenderer).toBeUndefined();
});
