import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BirdEasterEgg } from './BirdEasterEgg';
import { useEasterEggStore } from '../stores/easterEggStore';

vi.mock('react-dom', () => ({ createPortal: (children: unknown) => children }));
vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));

let view: ReactTestRenderer;
let resize: () => void;
let disconnect: ReturnType<typeof vi.fn>;
let lineTop: number;
let doc: EventTarget & { visibilityState: string };

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(Math, 'random').mockReturnValue(0);
  doc = Object.assign(new EventTarget(), { visibilityState: 'visible' });
  vi.stubGlobal('document', doc);
  vi.stubGlobal('window', new EventTarget());
  disconnect = vi.fn();
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
  vi.stubGlobal('localStorage', { getItem: vi.fn(), setItem: vi.fn() });
  useEasterEggStore.setState({ enabled: true });
  lineTop = 400;
});

afterEach(() => {
  act(() => view?.unmount());
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function mount() {
  const sidebarRef = {
    current: { getBoundingClientRect: () => ({ left: 16, top: 40 }) } as HTMLElement,
  };
  const perchRef = {
    current: { getBoundingClientRect: () => ({ right: 248, top: lineTop }) } as HTMLDivElement,
  };
  act(() => {
    view = create(<BirdEasterEgg sidebarRef={sidebarRef} perchRef={perchRef} />, {
      createNodeMock: () => ({
        getBoundingClientRect: () => ({ left: 40, top: 42, width: 28, height: 28 }),
      }),
    });
  });
}

function phase() {
  return (
    view.root.findAllByProps({ className: 'bird-easter-egg-overlay' })[0]?.props['data-phase'] ??
    'idle'
  );
}

function logo() {
  return view.root.findAllByType('button')[0];
}
function bird() {
  return view.root.findByProps({ className: 'bird-easter-egg-bird' });
}
function idleFrame() {
  return view.root.findByProps({ src: '/brand/animations/echobird-standing-idle.png' });
}
function actionFrame() {
  return bird().findAllByType('img')[2];
}
function standingLayer() {
  return view.root.findByProps({ className: 'bird-easter-egg-standing' });
}
function deferredDecode() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { resolve, reject, image: { isConnected: true, decode: vi.fn(() => promise) } };
}
async function readyAction() {
  const frame = actionFrame();
  await act(async () => {
    frame.props.onLoad({ currentTarget: { isConnected: true, decode: () => Promise.resolve() } });
  });
  return frame;
}
function advance(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}
function launch() {
  act(() => {
    logo().props.onClick();
  });
}
function arrive() {
  [220, 280, 900, 2400, 220].forEach(advance);
}
function returnHome() {
  [220, 2400, 900, 280, 220].forEach(advance);
}
function visibility(state: string) {
  act(() => {
    doc.visibilityState = state;
    doc.dispatchEvent(new Event('visibilitychange'));
  });
}

describe('BirdEasterEgg', () => {
  it.each([0, 0.99999])(
    'waits a random 30–60 minutes before automatically taking off (%s)',
    (random) => {
      vi.mocked(Math.random).mockReturnValue(random);
      mount();
      const delay = (30 + random * 30) * 60000;
      advance(delay - 1);
      expect(phase()).toBe('idle');
      advance(1);
      expect(phase()).toBe('logo-out');
      expect(localStorage.setItem).not.toHaveBeenCalled();
    }
  );

  it('fades the logo, hovers before moving, stands for 30 seconds, and restores the logo after returning', async () => {
    mount();
    launch();
    expect(phase()).toBe('logo-out');
    expect(logo().props.disabled).toBe(true);
    expect(view.root.findByProps({ src: '/brand/bird.png' }).props.style.opacity).toBe(0);
    advance(220);
    expect(phase()).toBe('flight-in');
    expect(view.root.findByProps({ className: 'bird-easter-egg-x' }).props.style.transform).toBe(
      'translateX(38px)'
    );
    advance(280);
    expect(phase()).toBe('home-hover');
    advance(899);
    expect(phase()).toBe('home-hover');
    advance(1);
    expect(phase()).toBe('outbound');
    expect(view.root.findByProps({ className: 'bird-easter-egg-x' }).props.style.transform).toBe(
      'translateX(210.6px)'
    );
    advance(2400);
    expect(phase()).toBe('landing');
    advance(220);
    expect(phase()).toBe('standing');
    expect(bird().props.disabled).toBe(false);
    advance(29999);
    expect(phase()).toBe('standing');
    const standingFrame = await readyAction();
    advance(1);
    expect(phase()).toBe('takeoff');
    expect(actionFrame()).toBe(standingFrame);
    expect(standingFrame.props.style.opacity).toBe(1);
    expect(standingLayer().props.style.opacity).toBe(0);
    advance(220);
    expect(phase()).toBe('inbound');
    expect(view.root.findByProps({ className: 'bird-easter-egg-x' }).props.style.transform).toBe(
      'translateX(38px)'
    );
    expect(
      view.root.findByProps({ src: '/brand/animations/echobird-flight-loop.webp' }).props.style
        .transform
    ).toBe('scaleX(-1)');
    advance(2400);
    expect(phase()).toBe('return-hover');
    advance(900);
    expect(phase()).toBe('flight-out');
    advance(280);
    expect(phase()).toBe('logo-in');
    expect(view.root.findByProps({ src: '/brand/bird.png' }).props.style.opacity).toBe(1);
    advance(220);
    expect(phase()).toBe('idle');
    expect(vi.getTimerCount()).toBe(1);
  });

  it('plays all three independent standing actions with quiet pauses between them', async () => {
    mount();
    launch();
    arrive();
    expect(idleFrame().props.style.opacity).toBe(1);
    advance(600);
    expect((await readyAction()).props.src).toContain('standing-look.webp');
    advance(2000);
    expect(idleFrame().props.style.opacity).toBe(1);
    expect(actionFrame()).toBeUndefined();
    advance(600);
    expect((await readyAction()).props.src).toContain('standing-preen.webp');
    advance(3000);
    expect(idleFrame().props.style.opacity).toBe(1);
    advance(600);
    expect((await readyAction()).props.src).toContain('standing-ruffle.webp');
  });

  it('keeps the already decoded idle frame mounted while the next action loads', () => {
    mount();
    launch();
    arrive();
    const idle = view.root.findByProps({ src: '/brand/animations/echobird-standing-idle.png' });
    advance(600);
    expect(view.root.findByProps({ src: '/brand/animations/echobird-standing-idle.png' })).toBe(
      idle
    );
    expect(idle.props.style.opacity).toBe(1);
    const pending = view.root.findByProps({ src: '/brand/animations/echobird-standing-look.webp' });
    expect(pending.props.style.opacity).toBe(0);
  });

  it('keeps the bird opaque until decoding completes, then switches to the ready action and back without replacing idle', async () => {
    mount();
    launch();
    arrive();
    const idle = idleFrame();
    advance(600);
    const pending = actionFrame();
    const decode = deferredDecode();
    act(() => {
      pending.props.onLoad({ currentTarget: decode.image });
    });
    advance(250);
    expect(idleFrame()).toBe(idle);
    expect(idle.props.style.opacity).toBe(1);
    expect(pending.props.style.opacity).toBe(0);
    expect(standingLayer().props.style.opacity).toBe(1);
    await act(async () => {
      decode.resolve();
    });
    expect(idle.props.style.opacity).toBe(0);
    expect(actionFrame()).toBe(pending);
    expect(pending.props.style.opacity).toBe(1);
    advance(1750);
    expect(idleFrame()).toBe(idle);
    expect(idle.props.style.opacity).toBe(1);
    expect(actionFrame()).toBeUndefined();
  });

  it('ignores an earlier decode completion and starts a fresh image when an action repeats', async () => {
    mount();
    launch();
    arrive();
    advance(600);
    const first = actionFrame();
    const firstSource = first.props.src;
    const decode = deferredDecode();
    act(() => {
      first.props.onLoad({ currentTarget: decode.image });
    });
    advance(2000);
    advance(600);
    const newer = actionFrame();
    await act(async () => {
      decode.resolve();
    });
    expect(actionFrame()).toBe(newer);
    expect(newer.props.style.opacity).toBe(0);
    expect(idleFrame().props.style.opacity).toBe(1);
    await readyAction();
    advance(3000);
    advance(600);
    await readyAction();
    advance(2000);
    advance(600);
    expect(actionFrame()).not.toBe(first);
    expect(actionFrame().props.src).toBe(firstSource);
    expect(actionFrame().props.style.opacity).toBe(0);
    expect(idleFrame().props.style.opacity).toBe(1);
  });

  it('recalls during decoding by fading the displayed idle frame and ignoring the pending action', async () => {
    mount();
    launch();
    arrive();
    advance(600);
    const idle = idleFrame();
    const decode = deferredDecode();
    act(() => {
      actionFrame().props.onLoad({ currentTarget: decode.image });
    });
    act(() => {
      bird().props.onClick();
    });
    expect(phase()).toBe('takeoff');
    expect(idleFrame()).toBe(idle);
    expect(idle.props.style.opacity).toBe(1);
    expect(standingLayer().props.style.opacity).toBe(0);
    expect(actionFrame()).toBeUndefined();
    await act(async () => {
      decode.resolve();
    });
    expect(actionFrame()).toBeUndefined();
    returnHome();
    expect(phase()).toBe('idle');
    expect(vi.getTimerCount()).toBe(1);
  });

  it.each(['disabled', 'hidden'])(
    'ignores a late decode rejection after the image is disconnected by %s',
    async (reason) => {
      mount();
      launch();
      arrive();
      advance(600);
      const decode = deferredDecode();
      act(() => {
        actionFrame().props.onLoad({ currentTarget: decode.image });
      });
      if (reason === 'disabled')
        act(() => {
          useEasterEggStore.getState().setEnabled(false);
        });
      else visibility('hidden');
      decode.image.isConnected = false;
      if (reason === 'disabled')
        act(() => {
          useEasterEggStore.getState().setEnabled(true);
        });
      else visibility('visible');
      await act(async () => {
        decode.reject(new Error('old decode aborted'));
      });
      expect(phase()).toBe('idle');
      expect(logo().props.disabled).toBe(false);
      expect(vi.getTimerCount()).toBe(1);
      launch();
      expect(phase()).toBe('logo-out');
    }
  );

  it('restores the static logo on a current decode failure and cleans up pending decoding on unmount', async () => {
    mount();
    launch();
    arrive();
    advance(600);
    const failed = deferredDecode();
    act(() => {
      actionFrame().props.onLoad({ currentTarget: failed.image });
    });
    await act(async () => {
      failed.reject(new Error('invalid image'));
    });
    expect(phase()).toBe('idle');
    expect(vi.getTimerCount()).toBe(0);
    act(() => {
      view.unmount();
    });
    mount();
    launch();
    arrive();
    advance(600);
    const late = deferredDecode();
    act(() => {
      actionFrame().props.onLoad({ currentTarget: late.image });
    });
    act(() => {
      view.unmount();
    });
    late.image.isConnected = false;
    await act(async () => {
      late.resolve();
    });
    expect(vi.getTimerCount()).toBe(0);
  });

  it('returns early when the perched bird is clicked and starts a fresh automatic interval afterwards', () => {
    mount();
    launch();
    arrive();
    act(() => {
      bird().props.onClick();
    });
    expect(phase()).toBe('takeoff');
    returnHome();
    expect(phase()).toBe('idle');
    advance(30 * 60000 - 1);
    expect(phase()).toBe('idle');
    advance(1);
    expect(phase()).toBe('logo-out');
  });

  it('fades the currently playing standing action out when recalled and begins the next visit at rest', async () => {
    mount();
    launch();
    arrive();
    advance(600);
    const standingFrame = await readyAction();
    expect(standingFrame.props.src).toContain('standing-look.webp');
    act(() => {
      bird().props.onClick();
    });
    expect(actionFrame()).toBe(standingFrame);
    expect(standingFrame.props.style.opacity).toBe(1);
    expect(standingLayer().props.style.opacity).toBe(0);
    expect(bird().findAllByType('img')[0].props.style.opacity).toBe(1);
    returnHome();
    launch();
    arrive();
    expect(bird().findAllByType('img')[1].props.src).toContain('standing-idle.png');
  });

  it.each(['idle', 'outbound', 'standing', 'inbound'])(
    'disabling during %s cancels the entire sequence and both triggers',
    (when) => {
      mount();
      if (when !== 'idle') {
        launch();
        [220, 280, 900].forEach(advance);
        if (when === 'standing' || when === 'inbound') [2400, 220].forEach(advance);
        if (when === 'inbound') [30000, 220].forEach(advance);
      }
      expect(phase()).toBe(when);
      act(() => {
        useEasterEggStore.getState().setEnabled(false);
      });
      expect(phase()).toBe('idle');
      expect(logo().props.disabled).toBe(true);
      expect(view.root.findByProps({ src: '/brand/bird.png' }).props.style.opacity).toBe(1);
      expect(vi.getTimerCount()).toBe(0);
      advance(2 * 60 * 60000);
      expect(phase()).toBe('idle');
      act(() => {
        useEasterEggStore.getState().setEnabled(true);
      });
      advance(30 * 60000 - 1);
      expect(phase()).toBe('idle');
    }
  );

  it('restores the logo while hidden and does not catch up overdue automatic trips on focus', () => {
    mount();
    launch();
    arrive();
    visibility('hidden');
    expect(phase()).toBe('idle');
    expect(vi.getTimerCount()).toBe(0);
    advance(2 * 60 * 60000);
    visibility('visible');
    expect(phase()).toBe('idle');
    advance(30 * 60000 - 1);
    expect(phase()).toBe('idle');
  });

  it('keeps the feet on the separator after resizing and leaves only the perched bird interactive', () => {
    mount();
    launch();
    arrive();
    const y = () => view.root.findByProps({ className: 'bird-easter-egg-y' }).props.style.transform;
    expect(y()).toBe('translateY(338.7px)');
    lineTop = 450;
    act(() => {
      resize();
    });
    expect(y()).toBe('translateY(388.7px)');
    expect(bird().props.style.pointerEvents).toBe('auto');
    expect(bird().props.style.clipPath).toBe('inset(30% 16% 12% 20%)');
    act(() => {
      bird().props.onClick();
    });
    expect(bird().props.style.pointerEvents).toBe('none');
    expect(bird().props.style.clipPath).toBeUndefined();
  });

  it('falls back to the static logo if a bundled asset fails and cleans up all work on unmount', () => {
    mount();
    launch();
    act(() => {
      bird().findAllByType('img')[0].props.onError();
    });
    expect(phase()).toBe('idle');
    expect(vi.getTimerCount()).toBe(0);
    act(() => {
      view.unmount();
    });
    expect(disconnect).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('cleans up pending standing actions and the return deadline on unmount', () => {
    mount();
    launch();
    arrive();
    expect(vi.getTimerCount()).toBe(2);
    act(() => {
      view.unmount();
    });
    expect(vi.getTimerCount()).toBe(0);
  });
});
