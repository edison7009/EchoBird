import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { RustSupplyCanvas } from './RustSupplyCanvas';
import { loadPetRenderer } from './petRenderer.js';

const copy = vi.hoisted(() => ({ prefix: '' }));
vi.mock('./petRenderer.js', () => ({ loadPetRenderer: vi.fn() }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => copy.prefix + key }),
}));
const native = {
  add_art: vi.fn(),
  set_logo: vi.fn(),
  set_copy: vi.fn(),
  resize: vi.fn(),
  set_reduced_motion: vi.fn(),
  pointer_move: vi.fn(),
  pointer_down: vi.fn(() => true),
  pointer_up: vi.fn(() => 1),
  leave: vi.fn(),
  open: vi.fn(),
  timing: () => [100, 200],
  skip_to_cards: vi.fn(),
  focus: vi.fn(),
  pick: vi.fn(),
  picked_index: () => 3,
  reveal: vi.fn(),
  show_revealed: vi.fn(),
  frame: vi.fn(() => 0),
  dispose: vi.fn(),
  free: vi.fn(),
};
let view: ReactTestRenderer;
let callbacks: Map<number, FrameRequestCallback>;
let nextFrame: number;
const handlers = {
  onOpen: vi.fn(),
  onGesture: vi.fn(),
  onPick: vi.fn(),
  onTearCue: vi.fn(),
  onReady: vi.fn(),
  onRevealMidpoint: vi.fn(),
  onRevealFinished: vi.fn(),
  onError: vi.fn(),
};
const canvas = {
  getBoundingClientRect: () => ({ width: 900, height: 600, left: 0, top: 0 }),
  setPointerCapture: vi.fn(),
};
const media = { matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() };
async function render(phase: 'sealed' | 'opening' | 'revealing' = 'sealed') {
  await act(async () => {
    view = create(
      <RustSupplyCanvas phase={phase} selected={3} kind="growth" value="+50M" {...handlers} />,
      { createNodeMock: () => canvas }
    );
  });
}
function frame(bits: number) {
  native.frame.mockReturnValueOnce(bits);
  const [id, callback] = callbacks.entries().next().value!;
  callbacks.delete(id);
  act(() => callback(100));
}
beforeEach(() => {
  vi.clearAllMocks();
  copy.prefix = '';
  callbacks = new Map();
  nextFrame = 0;
  vi.mocked(loadPetRenderer).mockResolvedValue({
    SupplyScene: class {
      constructor() {
        return { ...native };
      }
    },
  });
  vi.stubGlobal('document', {
    baseURI: 'http://localhost/',
    hidden: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  vi.stubGlobal('window', { matchMedia: () => media, devicePixelRatio: 2 });
  vi.stubGlobal(
    'Image',
    class {
      onload?: () => void;
      set src(_value: string) {
        queueMicrotask(() => this.onload?.());
      }
    }
  );
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    }
  );
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
    callbacks.set(++nextFrame, cb);
    return nextFrame;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => callbacks.delete(id));
});
afterEach(() => {
  act(() => view?.unmount());
  vi.unstubAllGlobals();
});
it('sends no reward text to the native scene until an accepted pick', async () => {
  await render();
  expect(native.add_art).toHaveBeenCalledTimes(22);
  expect(native.resize).toHaveBeenCalledWith(900, 600, 2);
  expect(native.reveal).not.toHaveBeenCalled();
  expect(native.show_revealed).not.toHaveBeenCalled();
  act(() =>
    view.update(
      <RustSupplyCanvas phase="revealing" selected={3} kind="growth" value="+50M" {...handlers} />
    )
  );
  expect(native.reveal).toHaveBeenCalledExactlyOnceWith(
    0,
    'pets.reward.growth',
    '+50M',
    'pets.reward.growthDetail',
    'GROWTH'
  );
});
it('unlocks drag audio on press, then reports commitment in the release gesture once', async () => {
  await render();
  const c = view.root.findByType('canvas');
  const event = {
    button: 0,
    isPrimary: true,
    clientX: 10,
    clientY: 20,
    currentTarget: canvas,
    pointerId: 1,
  };
  act(() => c.props.onPointerDown(event));
  expect(handlers.onGesture).toHaveBeenCalledOnce();
  act(() => {
    c.props.onPointerUp(event);
    c.props.onPointerUp(event);
  });
  expect(handlers.onOpen).toHaveBeenCalledExactlyOnceWith(200, 100);
  frame(1 | 4);
  expect(handlers.onTearCue).toHaveBeenCalledOnce();
  expect(handlers.onReady).toHaveBeenCalledOnce();
});
it('delivers compressed reveal events in order and releases GPU resources on exit', async () => {
  await render();
  const order: string[] = [];
  handlers.onPick.mockImplementation(() => order.push('pick'));
  handlers.onRevealMidpoint.mockImplementation(() => order.push('mid'));
  handlers.onRevealFinished.mockImplementation(() => order.push('done'));
  frame(8 | 16 | 32);
  expect(order).toEqual(['pick', 'mid', 'done']);
  act(() => view.unmount());
  expect(callbacks.size).toBe(0);
  expect(native.dispose).toHaveBeenCalledOnce();
  expect(native.free).toHaveBeenCalledOnce();
});
it('stops drawing when hidden and cancels an interrupted pointer drag', async () => {
  await render();
  const c = view.root.findByType('canvas');
  const event = {
    button: 0,
    isPrimary: true,
    pointerId: 1,
    clientX: 10,
    clientY: 20,
    currentTarget: canvas,
  };
  act(() => c.props.onPointerDown(event));
  act(() => c.props.onPointerCancel(event));
  expect(native.leave).toHaveBeenCalledOnce();
  Object.assign(document, { hidden: true });
  frame(1 | 4);
  expect(native.frame).not.toHaveBeenCalled();
  expect(handlers.onTearCue).not.toHaveBeenCalled();
  expect(callbacks.size).toBe(0);
});

it('ignores right clicks and releases without an accepted primary press', async () => {
  await render();
  const c = view.root.findByType('canvas');
  const event = {
    button: 0,
    isPrimary: true,
    pointerId: 1,
    clientX: 10,
    clientY: 20,
    currentTarget: canvas,
  };
  act(() => {
    c.props.onPointerUp(event);
    c.props.onPointerDown({ ...event, button: 2 });
    c.props.onPointerUp({ ...event, button: 2 });
    c.props.onPointerDown({ ...event, isPrimary: false });
    c.props.onPointerUp({ ...event, isPrimary: false });
  });
  expect(native.pointer_up).not.toHaveBeenCalled();
  expect(handlers.onOpen).not.toHaveBeenCalled();
});
it('releases partially initialized native resources immediately on failure', async () => {
  native.set_copy.mockImplementationOnce(() => {
    throw new Error('texture unavailable');
  });
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  try {
    await render();
    expect(handlers.onError).toHaveBeenCalledOnce();
    expect(native.dispose).toHaveBeenCalledOnce();
    expect(native.free).toHaveBeenCalledOnce();
    act(() => view.unmount());
    expect(native.free).toHaveBeenCalledOnce();
  } finally {
    log.mockRestore();
  }
});

it('restores the current reveal on a replacement scene after changing language', async () => {
  await render('revealing');
  expect(native.reveal).toHaveBeenCalledOnce();
  copy.prefix = 'localized:';
  await act(async () => {
    view.update(
      <RustSupplyCanvas phase="revealing" selected={3} kind="growth" value="+50M" {...handlers} />
    );
  });
  expect(native.dispose).toHaveBeenCalledOnce();
  expect(native.reveal).toHaveBeenCalledTimes(2);
  expect(native.reveal.mock.lastCall?.[1]).toBe('localized:pets.reward.growth');
});
