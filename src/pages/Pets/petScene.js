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

// `options.reduced` overrides the system's reduced-motion setting (previews, tests).
export function createPetScene(canvas, options = {}) {
  if (!native) throw new Error('Call loadPetScene() before createPetScene()');
  const scene = new native.PetScene(canvas);
  const reduced = options.reduced ?? window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  scene.set_reduced_motion(reduced);
  const root = canvas.ownerDocument?.documentElement;
  const ratio = () => window.devicePixelRatio || 1;
  let current = { id: 1, kind: 'mecha', stage: 2, color: 0 };
  // Every animation gets a serial number; a frame from an older one (already
  // queued when it was cancelled) is ignored instead of finishing it.
  let serial = 0;
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
    serial++;
    cancelAnimationFrame(raf);
    raf = 0;
    mode = 'still';
    progress = 0;
  }
  // `impact` is the fraction of the span where the effect lands; `onImpact`
  // fires once when it is reached (for the sound), never after a cancel.
  function animate(nextMode, from, to, duration, done = () => {}, onProgress, impact, onImpact) {
    stop();
    mode = nextMode;
    progress = from;
    const token = serial;
    const cancelled = () => disposed || token !== serial;
    onProgress?.(0);
    if (cancelled()) return;
    let impacted = impact === undefined;
    const start = performance.now();
    const tick = (now) => {
      if (cancelled()) return;
      progress = Math.min(to, from + (now - start) / duration);
      const share = (progress - from) / (to - from);
      onProgress?.(share);
      if (cancelled()) return;
      if (!impacted && share >= impact) {
        impacted = true;
        onImpact?.();
        if (cancelled()) return;
      }
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
  // Same-form beats and where their impact lands (see pet_fx.rs).
  const TIERS = {
    absorb: { ms: 450 },
    level: { ms: 700, impact: 0.42 },
    milestone: { ms: 1300, impact: 0.42 },
    max: { ms: 2400, impact: 0.62 },
  };
  // Reduced motion: no flight, flash or spin, only a short steady glow.
  const span = (ms) => (reduced ? Math.min(ms, 450) : ms);
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
    // `done` still fires exactly once, when everything visible has finished;
    // `cues.onImpact` is optional and fires at the visual peak.
    upgrade(next, done, onProgress, cues = {}) {
      const finish = () => {
        current = { ...next };
        done();
      };
      if (next.stage === current.stage) {
        const tier =
          next.level === current.level
            ? 'absorb'
            : next.level >= 60
              ? 'max'
              : next.level % 10 === 0
                ? 'milestone'
                : 'level';
        const { ms, impact } = TIERS[tier];
        animate(tier, 0, 1, span(ms), finish, onProgress, impact, cues.onImpact);
        return;
      }
      // A form change: the transformation, then a short landing on the new
      // form (rings, confetti) before `done`.
      const stage = current.stage;
      animate(
        stage === 0 ? 'crack' : stage === 1 ? 'hatch' : 'grow',
        stage === 1 ? crackedEggProgress : stage === 3 ? 0.5 : 0,
        stage === 2 ? 0.5 : 1,
        span(stage === 0 ? 1400 : 2600),
        () => {
          current = { ...next };
          if (reduced) return done();
          animate('land', 0, 1, 450, done, undefined, 0, cues.onImpact);
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
