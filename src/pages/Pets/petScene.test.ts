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

it('ends the crack at the same cracked shell that level ten shows, then hatches from it', () => {
  renderer.show(egg(0));
  const done = vi.fn();
  renderer.upgrade(egg(1), done);
  expect(done).toHaveBeenCalledTimes(1);
  expect(calls).toContainEqual(['mode', 'crack', 1]);
  expect(last('show')).toEqual(['show', 'mecha', 1, 0]);
  expect(last('mode')).toEqual(['mode', 'still', 0]);
  calls = [];
  renderer.upgrade({ ...egg(2), level: 20 }, vi.fn());
  expect(modes()).toContain('hatch');
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
    let tick: FrameRequestCallback;
    const request = vi.fn((callback: FrameRequestCallback) => {
      tick = callback;
      return 1;
    });
    vi.stubGlobal('requestAnimationFrame', request);
    create(false);
    renderer.show(egg(from));
    const progress = vi.fn(),
      done = vi.fn();
    const start = performance.now();
    renderer.upgrade(egg(to), done, progress);
    expect(progress).toHaveBeenLastCalledWith(0);
    tick!(start + 100);
    expect(progress.mock.lastCall![0]).toBeGreaterThan(0);
    expect(progress.mock.lastCall![0]).toBeLessThan(1);
    const frames = request.mock.calls.length;
    tick!(start + 3000);
    expect(progress).toHaveBeenLastCalledWith(1);
    expect(done).toHaveBeenCalledTimes(1);
    expect(request).toHaveBeenCalledTimes(frames);
  }
);

it('does not restart an idle gesture or interrupt an upgrade when more idle requests arrive', () => {
  renderer.dispose();
  let tick: FrameRequestCallback;
  const request = vi.fn((callback: FrameRequestCallback) => {
    tick = callback;
    return 1;
  });
  vi.stubGlobal('requestAnimationFrame', request);
  create(false);
  renderer.show(egg(0));
  renderer.interact();
  renderer.interact();
  renderer.interact();
  expect(request).toHaveBeenCalledTimes(1);
  const done = vi.fn(),
    start = performance.now();
  renderer.upgrade(egg(1), done);
  const frames = request.mock.calls.length;
  renderer.interact();
  expect(request).toHaveBeenCalledTimes(frames);
  tick!(start + 3000);
  expect(done).toHaveBeenCalledOnce();
  renderer.interact();
  expect(request).toHaveBeenCalledTimes(frames + 1);
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
