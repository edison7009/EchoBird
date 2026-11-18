import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createPetScene, loadPetScene } from './petScene.js';
import type { PetScene } from './petScene';

// The shell, core, hatch and growth geometry are tested in Rust
// (`echo-supply-renderer`: pet_models, pet_pose). These tests cover the
// JavaScript side: what it asks the WebAssembly scene to draw, and when.
type Call = [string, ...unknown[]];
let calls: Call[] = [];
const scenes: FakeScene[] = [];
class FakeScene {
  dispose = vi.fn();
  free = vi.fn();
  constructor() {
    scenes.push(this);
  }
  set_reduced_motion() {}
  set_daylight() {}
  rotate(dx: number, dy: number) {
    calls.push(['rotate', dx, dy]);
  }
  show(kind: string, stage: number, color: number) {
    calls.push(['show', kind, stage, color]);
  }
  set_mode(mode: string, progress: number) {
    calls.push(['mode', mode, progress]);
  }
  render() {
    calls.push(['render']);
  }
  thumbnail(kind: string, stage: number, color: number) {
    calls.push(['thumbnail']);
    return `data:${kind}:${stage}:${color}`;
  }
}
await loadPetScene(() => Promise.resolve({ default: async () => {}, PetScene: FakeScene }));

let renderer: PetScene;
const canvas = { clientWidth: 500, clientHeight: 400 } as HTMLCanvasElement;
function create(reduced: boolean) {
  vi.stubGlobal('window', { matchMedia: () => ({ matches: reduced }), devicePixelRatio: 1 });
  renderer = createPetScene(canvas);
}
beforeEach(() => {
  calls = [];
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    }
  );
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  vi.stubGlobal('requestAnimationFrame', vi.fn());
  create(true);
});
afterEach(() => {
  renderer?.dispose();
  vi.unstubAllGlobals();
});

const egg = (stage: number) => ({
  id: 1,
  kind: 'mecha' as const,
  stage,
  level: stage * 10,
  color: 0,
});
const last = (name: string) => calls.filter((c) => c[0] === name).slice(-1)[0];
const modes = () => calls.filter((c) => c[0] === 'mode').map((c) => c[1]);

it('draws the intact and the cracked egg on request without an idle animation loop', () => {
  renderer.show(egg(0));
  expect(last('show')).toEqual(['show', 'mecha', 0, 0]);
  renderer.show({ ...egg(1), color: 1 });
  expect(last('show')).toEqual(['show', 'mecha', 1, 1]);
  expect(last('mode')).toEqual(['mode', 'still', 0]);
  expect(calls[calls.length - 1]).toEqual(['render']);
  expect(requestAnimationFrame).not.toHaveBeenCalled();
});

/** Takes over animation frames and runs them on a fake clock. */
function clock() {
  let tick: FrameRequestCallback | undefined;
  const request = vi.fn((callback: FrameRequestCallback) => {
    tick = callback;
    return 1;
  });
  vi.stubGlobal('requestAnimationFrame', request);
  vi.stubGlobal('cancelAnimationFrame', () => {
    tick = undefined;
  });
  const start = performance.now();
  return {
    request,
    at(ms: number) {
      const callback = tick;
      tick = undefined;
      callback?.(start + ms);
    },
    /** Runs frames every 16 ms up to `ms`; true while a frame is still pending. */
    run(ms: number) {
      for (let t = 16; t <= ms; t += 16) this.at(t);
      return tick !== undefined;
    },
  };
}

it('ends the crack at the same cracked shell that level ten shows, then hatches from it', () => {
  renderer.show(egg(0));
  const time = clock();
  const done = vi.fn();
  renderer.upgrade(egg(1), done);
  expect(done).not.toHaveBeenCalled();
  // Reduced motion: a short, calm version, with no landing afterwards.
  expect(time.run(600)).toBe(false);
  expect(done).toHaveBeenCalledTimes(1);
  expect(modes()).toContain('crack');
  expect(modes()).not.toContain('land');
  expect(last('show')).toEqual(['show', 'mecha', 1, 0]);
  expect(last('mode')).toEqual(['mode', 'still', 0]);
  calls = [];
  renderer.upgrade({ ...egg(2), level: 20 }, vi.fn());
  expect(modes()).toContain('hatch');
  time.run(600);
  expect(last('show')).toEqual(['show', 'mecha', 2, 0]);
});

it.each([
  [0, 0],
  [0, 1],
  [1, 2],
  [2, 3],
  [3, 4],
])(
  'reports normalized spending progress through stage %i → %i, then stops requesting frames',
  (from, to) => {
    renderer.dispose();
    create(false);
    const time = clock();
    renderer.show(egg(from));
    const progress = vi.fn(),
      done = vi.fn();
    renderer.upgrade(egg(to), done, progress);
    expect(progress).toHaveBeenLastCalledWith(0);
    time.at(100);
    expect(progress.mock.lastCall![0]).toBeGreaterThan(0);
    expect(progress.mock.lastCall![0]).toBeLessThan(1);
    expect(time.run(4000)).toBe(false);
    expect(progress).toHaveBeenLastCalledWith(1);
    expect(done).toHaveBeenCalledTimes(1);
  }
);

it('does not restart an idle gesture or interrupt an upgrade when more idle requests arrive', () => {
  renderer.dispose();
  create(false);
  const time = clock();
  renderer.show(egg(0));
  renderer.interact();
  renderer.interact();
  renderer.interact();
  expect(time.request).toHaveBeenCalledTimes(1);
  time.run(2000);
  const done = vi.fn();
  renderer.upgrade(egg(1), done);
  const frames = time.request.mock.calls.length;
  renderer.interact();
  expect(time.request).toHaveBeenCalledTimes(frames);
  time.run(6000);
  expect(done).toHaveBeenCalledOnce();
  const after = time.request.mock.calls.length;
  renderer.interact();
  expect(time.request).toHaveBeenCalledTimes(after + 1);
});

it('picks the feedback from the target level: absorb, level, milestone, form change, max', () => {
  renderer.dispose();
  create(false);
  const time = clock();
  const pet = (level: number, stage: number) => ({ ...egg(stage), level });
  const played = (from: ReturnType<typeof pet>, to: ReturnType<typeof pet>) => {
    renderer.show(from);
    calls = [];
    renderer.upgrade(to, vi.fn());
    time.run(4000);
    return [...new Set(modes())].filter((m) => m !== 'still');
  };
  expect(played(pet(12, 1), pet(12, 1))).toEqual(['absorb']);
  expect(played(pet(12, 1), pet(13, 1))).toEqual(['level']);
  expect(played(pet(39, 3), pet(40, 3))).toEqual(['milestone']);
  expect(played(pet(59, 4), pet(60, 4))).toEqual(['max']);
  expect(played(pet(29, 2), pet(30, 3))).toEqual(['grow', 'land']);
});

it('cues the impact once for a level and never for absorbed growth', () => {
  renderer.dispose();
  create(false);
  const time = clock();
  const onImpact = vi.fn();
  renderer.show({ ...egg(1), level: 12 });
  renderer.upgrade({ ...egg(1), level: 12 }, vi.fn(), undefined, { onImpact });
  time.run(1000);
  expect(onImpact).not.toHaveBeenCalled();
  renderer.upgrade({ ...egg(1), level: 13 }, vi.fn(), undefined, { onImpact });
  time.run(1000);
  expect(onImpact).toHaveBeenCalledTimes(1);
});

it('cancels both the completion and the impact when the pet is switched mid-upgrade', () => {
  renderer.dispose();
  create(false);
  const time = clock();
  const done = vi.fn(),
    onImpact = vi.fn();
  renderer.show({ ...egg(4), level: 59 });
  renderer.upgrade({ ...egg(4), level: 60 }, done, undefined, { onImpact });
  time.run(500);
  renderer.show({ ...egg(2), level: 21 });
  expect(time.run(6000)).toBe(false);
  expect(done).not.toHaveBeenCalled();
  expect(onImpact).not.toHaveBeenCalled();
  expect(last('show')).toEqual(['show', 'mecha', 2, 0]);
});

it('ignores a frame that was already queued when the upgrade was cancelled', () => {
  renderer.dispose();
  create(false);
  let queued: FrameRequestCallback | undefined;
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    queued = callback;
    return 1;
  });
  const done = vi.fn();
  renderer.show({ ...egg(1), level: 12 });
  const start = performance.now();
  renderer.upgrade({ ...egg(1), level: 13 }, done);
  const stale = queued!;
  renderer.show({ ...egg(2), level: 21 });
  stale(start + 5000);
  expect(done).not.toHaveBeenCalled();
  expect(last('show')).toEqual(['show', 'mecha', 2, 0]);
});

it('keeps reduced motion short and skips the landing', () => {
  const time = clock();
  const done = vi.fn();
  renderer.show({ ...egg(2), level: 29 });
  calls = [];
  renderer.upgrade({ ...egg(3), level: 30 }, done);
  time.run(450);
  expect(done).toHaveBeenCalledTimes(1);
  expect(modes()).not.toContain('land');
});

it('caches each thumbnail and puts the main view back after drawing one', () => {
  renderer.show(egg(4));
  const url = renderer.thumbnail({ ...egg(4), kind: 'fox', color: 3 });
  expect(url).toBe('data:fox:4:3');
  expect(calls[calls.length - 1]).toEqual(['render']);
  expect(renderer.thumbnail({ ...egg(4), kind: 'fox', color: 3 })).toBe(url);
  expect(calls.filter((c) => c[0] === 'thumbnail')).toHaveLength(1);
});

it('passes drags through and frees the WebAssembly scene exactly once', () => {
  renderer.rotate(12, -4);
  expect(last('rotate')).toEqual(['rotate', 12, -4]);
  const scene = scenes[scenes.length - 1];
  renderer.dispose();
  renderer.dispose();
  expect(scene.dispose).toHaveBeenCalledTimes(1);
  expect(scene.free).toHaveBeenCalledTimes(1);
});

it.each(['initial progress', 'progress', 'impact'] as const)(
  'does not resume an upgrade cancelled inside its %s callback',
  (phase) => {
    renderer.dispose();
    create(false);
    const time = clock();
    const done = vi.fn();
    const other = { ...egg(2), level: 21 };
    const cancel = () => renderer.show(other);
    const onImpact = vi.fn(() => {
      if (phase === 'impact') cancel();
    });
    renderer.show({ ...egg(1), level: 12 });
    renderer.upgrade(
      { ...egg(1), level: 13 },
      done,
      (progress) => {
        if (
          (phase === 'initial progress' && progress === 0) ||
          (phase === 'progress' && progress > 0)
        )
          cancel();
      },
      { onImpact }
    );
    expect(time.run(5000)).toBe(false);
    expect(done).not.toHaveBeenCalled();
    expect(onImpact).toHaveBeenCalledTimes(phase === 'impact' ? 1 : 0);
    expect(last('show')).toEqual(['show', 'mecha', 2, 0]);
  }
);
