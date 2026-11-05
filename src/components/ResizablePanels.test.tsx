import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { useEffect, useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ResizablePanels } from './ResizablePanels';

vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

let view: ReactTestRenderer;
let available: number;
let resize: () => void;
let events: EventTarget;
let saved: Map<string, string>;
const key = 'echobird-right-panel-width';
const disconnect = vi.fn();
const write = vi.fn();

beforeEach(() => {
  available = 1000;
  events = new EventTarget();
  saved = new Map();
  write.mockReset().mockImplementation((key: string, value: string) => saved.set(key, value));
  disconnect.mockReset();
  vi.stubGlobal('window', events);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: write,
  });
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resize = callback;
      }
      observe() {}
      disconnect = disconnect;
    }
  );
});

afterEach(() => {
  act(() => view?.unmount());
  vi.unstubAllGlobals();
});

function mount(center = <main>Center</main>) {
  act(() => {
    view = create(
      <ResizablePanels>
        {center}
        <aside>Right</aside>
      </ResizablePanels>,
      { createNodeMock: () => ({ getBoundingClientRect: () => ({ width: available }) }) }
    );
  });
}
const handle = () => view.root.findByProps({ role: 'separator' });
const width = () => handle().props['aria-valuenow'];
function pointer(type: string, x = 700, pointerId = 1) {
  act(() => {
    events.dispatchEvent(Object.assign(new Event(type), { clientX: x, pointerId }));
  });
}
function start() {
  act(() =>
    handle().props.onPointerDown({
      button: 0,
      clientX: 700,
      pointerId: 1,
      preventDefault: vi.fn(),
      currentTarget: {
        setPointerCapture: vi.fn(),
        hasPointerCapture: () => true,
        releasePointerCapture: vi.fn(),
      },
    })
  );
}

it.each([undefined, 'NaN', 'Infinity', '0', '9999', '340'])(
  'loads valid local widths without writing (%s)',
  (value) => {
    if (value !== undefined) saved.set(key, value);
    mount();
    expect(width()).toBe(value === '340' ? 340 : 300);
    expect(write).not.toHaveBeenCalled();
  }
);

it('resizes live, clamps both sides and only saves after the matching pointer finishes', () => {
  mount();
  start();
  pointer('pointermove', 650);
  expect(width()).toBe(350);
  expect(write).not.toHaveBeenCalled();
  pointer('pointermove', 900, 2);
  pointer('pointerup', 900, 2);
  expect(width()).toBe(350);
  pointer('pointermove', 0);
  expect(width()).toBe(520);
  pointer('pointermove', 1500);
  expect(width()).toBe(260);
  pointer('pointerup');
  expect(write).toHaveBeenCalledExactlyOnceWith(key, '260');
  pointer('pointermove', 600);
  expect(width()).toBe(260);
  act(() => view.unmount());
  mount();
  expect(width()).toBe(260);
});

it.each(['pointercancel', 'blur'])('cleans up a drag on %s', (event) => {
  mount();
  start();
  pointer('pointermove', 680);
  pointer(event);
  expect(write).toHaveBeenCalledExactlyOnceWith(key, '320');
  pointer('pointermove', 500);
  expect(width()).toBe(320);
});

it('clamps window/sidebar changes without losing the preferred width or writing settings', () => {
  saved.set(key, '500');
  mount();
  available = 700;
  act(() => resize());
  expect(width()).toBe(314);
  available = 600;
  act(() => resize());
  expect(width()).toBe(214);
  available = 1100;
  act(() => resize());
  expect(width()).toBe(500);
  expect(write).not.toHaveBeenCalled();
});

it('supports keyboard adjustments and double-click reset with unavailable storage', () => {
  mount();
  write.mockImplementation(() => {
    throw new Error('unavailable');
  });
  act(() => handle().props.onKeyDown({ key: 'ArrowLeft', preventDefault: vi.fn() }));
  expect(width()).toBe(316);
  act(() => handle().props.onKeyDown({ key: 'ArrowRight', preventDefault: vi.fn() }));
  expect(width()).toBe(300);
  act(() => handle().props.onDoubleClick());
  expect(width()).toBe(300);
});

it('retains page state while resizing and removes all listeners on unmount', () => {
  const mounted = vi.fn();
  function Center() {
    const [value, setValue] = useState(0);
    useEffect(() => {
      mounted();
    }, []);
    return <button onClick={() => setValue(value + 1)}>{value}</button>;
  }
  mount(<Center />);
  act(() => view.root.findAllByType('button')[0].props.onClick());
  start();
  pointer('pointermove', 650);
  expect(view.root.findAllByType('button')[0].children).toEqual(['1']);
  expect(mounted).toHaveBeenCalledOnce();
  act(() => view.unmount());
  pointer('pointermove', 600);
  pointer('pointerup');
  expect(write).not.toHaveBeenCalled();
  expect(disconnect).toHaveBeenCalledOnce();
});
