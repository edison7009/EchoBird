import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ViewModeTabGroup, type ViewModeTabPosition } from './ViewModeTabGroup';

const platform = vi.hoisted(() => ({ IS_MACOS: false }));
vi.mock('../utils/platform', () => platform);

let renderer: ReactTestRenderer;
let selected: {
  offsetLeft: number;
  offsetTop: number;
  offsetWidth: number;
  offsetHeight: number;
  getBoundingClientRect?: () => { left: number; top: number; width: number; height: number };
} | null;
let root: ReturnType<typeof makeRoot>;
let observers: {
  update: () => void;
  observe: ReturnType<typeof vi.fn>;
  disconnect: ReturnType<typeof vi.fn>;
}[];
const onSelect = vi.fn();
function makeRoot() {
  const values = new Map<string, string>();
  return {
    values,
    clientLeft: 1,
    clientTop: 1,
    dataset: {} as Record<string, string>,
    style: { setProperty: (key: string, value: string) => values.set(key, value) },
    querySelector: () => selected,
    querySelectorAll: () => (selected ? [selected] : []),
    getBoundingClientRect: vi.fn((): Record<string, string | number> => Object.fromEntries(values)),
  };
}
function render(positionRef?: { current: ViewModeTabPosition | null }) {
  const element = (
    <ViewModeTabGroup className="view-mode-tabs" role="tablist" positionRef={positionRef}>
      <button className="view-mode-tab" role="tab" aria-selected onClick={onSelect}>
        Selected
      </button>
      <button className="view-mode-tab" role="tab" aria-selected={false} onClick={onSelect}>
        Other
      </button>
    </ViewModeTabGroup>
  );
  act(() => {
    if (renderer) renderer.update(element);
    else
      renderer = create(element, { createNodeMock: (node) => (node.type === 'div' ? root : null) });
  });
}
beforeEach(() => {
  platform.IS_MACOS = false;
  selected = { offsetLeft: 0, offsetTop: 0, offsetWidth: 32, offsetHeight: 30 };
  root = makeRoot();
  observers = [];
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe = vi.fn();
      disconnect = vi.fn();
      constructor(update: () => void) {
        observers.push({ update, observe: this.observe, disconnect: this.disconnect });
      }
    }
  );
});
afterEach(() => {
  act(() => renderer?.unmount());
  renderer = undefined as unknown as ReactTestRenderer;
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it('moves one background to the selected button and follows its actual width and wrapped row', () => {
  render();
  expect(root.dataset.sliding).toBe('true');
  expect(root.values.get('--view-mode-x')).toBe('0px');
  selected = { offsetLeft: 96, offsetTop: 30, offsetWidth: 78, offsetHeight: 28 };
  render();
  expect(Object.fromEntries(root.values)).toEqual({
    '--view-mode-x': '96px',
    '--view-mode-y': '30px',
    '--view-mode-width': '78px',
    '--view-mode-height': '28px',
  });
  expect(onSelect).not.toHaveBeenCalled();
  expect(renderer.root.findAllByType('button')).toHaveLength(2);
});

it('keeps the last geometry while hidden, updates after resize and disconnects on exit', () => {
  render();
  const observer = observers[0];
  expect(observer.observe).toHaveBeenCalledWith(root);
  selected = { offsetLeft: 0, offsetTop: 0, offsetWidth: 0, offsetHeight: 0 };
  act(() => observer.update());
  expect(root.values.get('--view-mode-width')).toBe('32px');
  selected = { offsetLeft: 72, offsetTop: 0, offsetWidth: 64, offsetHeight: 34 };
  act(() => observer.update());
  expect(root.values.get('--view-mode-x')).toBe('72px');
  expect(root.values.get('--view-mode-width')).toBe('64px');
  act(() => renderer.unmount());
  expect(observer.disconnect).toHaveBeenCalledOnce();
  renderer = undefined as unknown as ReactTestRenderer;
});

it('commits the remembered starting position before moving when a keyed router panel remounts', () => {
  const positionRef = { current: null as ViewModeTabPosition | null };
  render(positionRef);
  expect(root.getBoundingClientRect).not.toHaveBeenCalled();
  act(() => renderer.unmount());
  renderer = undefined as unknown as ReactTestRenderer;
  root = makeRoot();
  selected = { offsetLeft: 128, offsetTop: 0, offsetWidth: 32, offsetHeight: 30 };
  render(positionRef);
  expect(root.getBoundingClientRect).toHaveReturnedWith({
    '--view-mode-x': '0px',
    '--view-mode-y': '0px',
    '--view-mode-width': '32px',
    '--view-mode-height': '30px',
  });
  expect(root.values.get('--view-mode-x')).toBe('128px');
  expect(positionRef.current).toEqual({ x: 128, y: 0, width: 32, height: 30 });
  expect(onSelect).not.toHaveBeenCalled();
});

it('retains the ordinary selected-button fallback until a visible selection can be measured', () => {
  vi.stubGlobal('ResizeObserver', undefined);
  selected = null;
  render();
  expect(root.dataset.sliding).toBeUndefined();
  selected = { offsetLeft: 42, offsetTop: 0, offsetWidth: 53, offsetHeight: 30 };
  render();
  expect(root.dataset.sliding).toBe('true');
  expect(root.values.get('--view-mode-x')).toBe('42px');
  selected = null;
  render();
  expect(root.dataset.sliding).toBeUndefined();
});

it('keeps fractional macOS highlight geometry flush with the selected tab inside the border', () => {
  platform.IS_MACOS = true;
  root.getBoundingClientRect.mockReturnValue({ left: 100.25, top: 20.5 });
  selected = {
    offsetLeft: 59,
    offsetTop: 0,
    offsetWidth: 66,
    offsetHeight: 34,
    getBoundingClientRect: () => ({ left: 160.75, top: 21.5, width: 66.375, height: 34.25 }),
  };
  render();
  expect(Object.fromEntries(root.values)).toEqual({
    '--view-mode-x': '59.5px',
    '--view-mode-y': '0px',
    '--view-mode-width': '66.375px',
    '--view-mode-height': '34.25px',
  });
  expect(onSelect).not.toHaveBeenCalled();
});
