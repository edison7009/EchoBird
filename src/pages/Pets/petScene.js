import { loadPetRenderer } from './petRenderer.js';

// EchoBird's voxel pet companions, drawn by the Rust renderer
// (`echo-supply-renderer`, compiled to WebAssembly) with WebGL2.
//
// The models, poses, hatch, growth, tints and framing live in Rust; this file
// keeps the scene's JavaScript interface and its animation timing, so the
// Pets page works exactly as before. Call `loadPetScene()` once before
// `createPetScene()`.

let loading;
let native;

/**
 * Loads the WebAssembly renderer once per session (shared with the supply
 * pack). `load` replaces the default loader, for previews and tests.
 */
export function loadPetScene(load = loadPetRenderer) {
  if (!loading) {
    loading = load()
      .then((module) => {
        native = module;
      })
      .catch((error) => {
        loading = undefined;
        throw error;
      });
  }
  return loading;
}

export function createPetScene(canvas) {
  if (!native) throw new Error('Call loadPetScene() before createPetScene()');
  const scene = new native.PetScene(canvas);
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  scene.set_reduced_motion(reduced);
  const root = canvas.ownerDocument?.documentElement;
  const ratio = () => window.devicePixelRatio || 1;
  let current = { id: 1, kind: 'mecha', stage: 2, color: 0 };
  let disposed = false,
    raf = 0,
    progress = 0,
    mode = 'still';
  const thumbnailCache = new Map();

  function render() {
    if (disposed) return;
    // A bright surrounding interface needs softer, fuller illumination.
    scene.set_daylight(root?.dataset.theme === 'light');
    scene.show(current.kind, current.stage, current.color);
    scene.set_mode(mode, progress);
    scene.render(canvas.clientWidth, canvas.clientHeight, ratio());
  }
  function stop() {
    cancelAnimationFrame(raf);
    raf = 0;
    mode = 'still';
    progress = 0;
  }
  function animate(nextMode, from, to, duration, done = () => {}, onProgress) {
    stop();
    mode = nextMode;
    progress = from;
    onProgress?.(0);
    if (reduced) {
      progress = to;
      onProgress?.(1);
      render();
      stop();
      done();
      render();
      return;
    }
    const start = performance.now();
    const tick = (now) => {
      if (disposed) return;
      progress = Math.min(to, from + (now - start) / duration);
      onProgress?.((progress - from) / (to - from));
      render();
      if (progress < to) raf = requestAnimationFrame(tick);
      else {
        stop();
        done();
        render();
      }
    };
    render();
    raf = requestAnimationFrame(tick);
  }
  const crackedEggProgress = 0.46;
  const observer = new ResizeObserver(render);
  observer.observe(canvas);
  const appearanceObserver = root ? new MutationObserver(render) : null;
  appearanceObserver?.observe(root, { attributes: true, attributeFilter: ['data-theme'] });
  return {
    show(pet) {
      stop();
      current = { ...pet };
      render();
    },
    upgrade(next, done, onProgress) {
      if (next.stage === current.stage) {
        animate(
          'level',
          0,
          1,
          600,
          () => {
            current = { ...next };
            done();
          },
          onProgress
        );
        return;
      }
      const stage = current.stage;
      animate(
        stage === 0 ? 'crack' : stage === 1 ? 'hatch' : 'grow',
        stage === 1 ? crackedEggProgress : stage === 3 ? 0.5 : 0,
        stage === 2 ? 0.5 : 1,
        stage === 0 ? 1400 : 2600,
        () => {
          current = { ...next };
          done();
        },
        onProgress
      );
    },
    interact() {
      if (mode !== 'still') return;
      animate('interact', 0, 1, 1400);
    },
    rotate(dx, dy) {
      scene.rotate(dx, dy);
      render();
    },
    thumbnail(pet) {
      const id = [pet.kind, pet.stage, pet.color].join(':');
      if (!thumbnailCache.has(id)) {
        thumbnailCache.set(id, scene.thumbnail(pet.kind, pet.stage, pet.color, ratio()));
        render();
      }
      return thumbnailCache.get(id);
    },
    dispose() {
      if (disposed) return;
      stop();
      disposed = true;
      observer.disconnect();
      appearanceObserver?.disconnect();
      thumbnailCache.clear();
      scene.dispose();
      scene.free();
    },
  };
}
