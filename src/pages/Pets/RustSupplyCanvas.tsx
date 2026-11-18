import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { useI18n } from '../../hooks/useI18n';
import type { RewardKind } from './petRewardRules';
import { loadPetRenderer } from './petRenderer.js';

/**
 * EchoBird's pet-supply animation: a black pack you tear open, five face-down
 * cards, and the picked card flipping to show the reward the app already saved.
 *
 * Drawing runs in Rust compiled to WebAssembly (`echo-supply-renderer`), into
 * this canvas with WebGL2; nothing crosses Tauri IPC per frame. The scene only
 * receives the reward's words after the dialog accepts a pick, so hovering or
 * focusing a card can never show it.
 */
type Phase = 'sealed' | 'opening' | 'choosing' | 'revealing' | 'revealed';

// Event bits returned by SupplyScene.frame (see scene.rs).
const EV_TEAR_CUE = 1;
const EV_READY = 4;
const EV_PICKED = 8;
const EV_REVEAL_MID = 16;
const EV_REVEAL_DONE = 32;

const KIND_INDEX: Record<RewardKind, number> = {
  growth: 0,
  double: 1,
  haste: 2,
  combo: 3,
  super: 4,
};
const CHARACTERS = ['echo', 'sprout', 'twin', 'zip', 'puff'] as const;
const LAYERS = ['body', 'eyes', 'joy', 'wince', 'smile', 'open'] as const;

interface NativeScene {
  add_art(character: number, layer: number, image: HTMLImageElement): void;
  set_logo(image: HTMLImageElement): void;
  set_copy(brand: string, kind: string, hint: string, label: string, count: string): void;
  resize(width: number, height: number, dpr: number): void;
  set_reduced_motion(reduced: boolean): void;
  pointer_move(x: number, y: number): void;
  pointer_down(x: number, y: number): boolean;
  pointer_up(x: number, y: number): number;
  leave(): void;
  open(): void;
  timing(): Float64Array | number[];
  skip_to_cards(): void;
  focus(index: number): void;
  pick(index: number): void;
  picked_index(): number;
  reveal(kind: number, name: string, value: string, detail: string, badge: string): void;
  show_revealed(kind: number, name: string, value: string, detail: string, badge: string): void;
  frame(now: number): number;
  dispose(): void;
  free(): void;
}
interface RustModule {
  default(input?: unknown): Promise<unknown>;
  SupplyScene: new (canvas: HTMLCanvasElement) => NativeScene;
}
interface Resources {
  module: RustModule;
  art: { character: number; layer: number; image: HTMLImageElement }[];
  logo: HTMLImageElement;
}

let resources: Promise<Resources> | undefined;

function loadImage(url: string) {
  const image = new Image();
  const ready = new Promise<HTMLImageElement>((resolve, reject) => {
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`Supply artwork unavailable: ${url}`));
  });
  image.src = url;
  return ready;
}

/** Loads the WASM module and EchoBird's own supply artwork once per session. */
function loadResources() {
  if (resources) return resources;
  resources = (async () => {
    const base = new URL(import.meta.env.BASE_URL, document.baseURI);
    const module = (await loadPetRenderer()) as RustModule;
    const slots = CHARACTERS.flatMap((name, character) =>
      LAYERS.map((file, layer) => ({ name, character, layer, file }))
    ).filter((slot) => !(slot.name === 'echo' && slot.layer >= 4)); // Echo's beak is its mouth
    const art = await Promise.all(
      slots.map(async (slot) => ({
        character: slot.character,
        layer: slot.layer,
        image: await loadImage(new URL(`pet-supply-art/${slot.name}-${slot.file}.webp`, base).href),
      }))
    );
    const logo = await loadImage(new URL('brand/bird.png', base).href);
    return { module, art, logo };
  })().catch((error: unknown) => {
    resources = undefined;
    throw error;
  });
  return resources;
}

export function RustSupplyCanvas({
  phase,
  selected,
  kind,
  value,
  onOpen,
  onGesture,
  onPick,
  onTearCue,
  onReady,
  onRevealMidpoint,
  onRevealFinished,
  onError,
}: {
  phase: Phase;
  selected: number;
  kind: RewardKind;
  value: string;
  /** The tear is committed, inside the user's gesture: ms until the cards are ready, and until the tear sound. */
  onOpen: (readyMs: number, cueMs: number) => void;
  /** A face-down card was confirmed. The reward itself does not depend on the index. */
  onPick: (index: number) => void;
  /** Unlock audio at pointerdown so a held drag can play its tear cue. */
  onGesture?: () => void;
  onTearCue?: () => void;
  onReady?: () => void;
  onRevealMidpoint?: () => void;
  onRevealFinished?: () => void;
  onError?: (error: unknown) => void;
}) {
  const { t } = useI18n();
  const canvas = useRef<HTMLCanvasElement>(null);
  const scene = useRef<NativeScene | null>(null);
  const revealed = useRef(false);
  const committed = useRef(false);
  const activePointer = useRef<number | null>(null);
  const [readyScene, setReadyScene] = useState<NativeScene | null>(null);
  const ready = readyScene !== null;
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);

  // Callbacks change identity on every render; the frame loop reads the latest.
  const handlers = useRef({
    onOpen,
    onGesture,
    onPick,
    onTearCue,
    onReady,
    onRevealMidpoint,
    onRevealFinished,
    onError,
  });
  useEffect(() => {
    handlers.current = {
      onOpen,
      onGesture,
      onPick,
      onTearCue,
      onReady,
      onRevealMidpoint,
      onRevealFinished,
      onError,
    };
  });

  const name = t(`pets.reward.${kind}`);
  const detail = t(`pets.reward.${kind}Detail`);
  const copyKey = JSON.stringify([
    'EchoBird',
    'TOKENPET',
    `${t('pets.reward.open')} →`,
    t('pets.reward.title'),
    t('pets.reward.choose'),
  ]);

  useEffect(() => {
    let cancelled = false;
    let frame = 0;
    let live: NativeScene | undefined;
    let resize: ResizeObserver | undefined;
    const media = window.matchMedia('(prefers-reduced-motion: reduce)');
    revealed.current = false;
    committed.current = false;
    activePointer.current = null;

    const draw = (now: number) => {
      frame = 0;
      if (cancelled || document.hidden || !live) return;
      const events = live.frame(now);
      const h = handlers.current;
      if (events & EV_TEAR_CUE) h.onTearCue?.();
      if (events & EV_READY) h.onReady?.();
      if (events & EV_PICKED) h.onPick(live.picked_index());
      if (events & EV_REVEAL_MID) h.onRevealMidpoint?.();
      if (events & EV_REVEAL_DONE) h.onRevealFinished?.();
      frame = requestAnimationFrame(draw);
    };
    const wake = () => {
      if (!frame && !cancelled && !document.hidden) frame = requestAnimationFrame(draw);
    };
    // A hidden window stops drawing; showing it again resumes.
    const visibility = () => {
      if (document.hidden) {
        cancelAnimationFrame(frame);
        frame = 0;
      } else wake();
    };
    const motion = () => live?.set_reduced_motion(media.matches);

    void loadResources()
      .then(({ module, art, logo }) => {
        if (cancelled || !canvas.current) return;
        live = new module.SupplyScene(canvas.current);
        scene.current = live;
        for (const slot of art) live.add_art(slot.character, slot.layer, slot.image);
        live.set_logo(logo);
        const [brand, kindLabel, hint, label, count] = JSON.parse(copyKey) as string[];
        live.set_copy(brand, kindLabel, hint, label, count);
        live.set_reduced_motion(media.matches);
        const fit = () => {
          const bounds = canvas.current?.getBoundingClientRect();
          if (bounds && live)
            live.resize(bounds.width, bounds.height, window.devicePixelRatio || 1);
        };
        fit();
        resize = new ResizeObserver(fit);
        resize.observe(canvas.current);
        document.addEventListener('visibilitychange', visibility);
        media.addEventListener('change', motion);
        setReadyScene(live);
        setError(false);
        wake();
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        if (scene.current === live) scene.current = null;
        live?.dispose();
        live?.free();
        live = undefined;
        resize?.disconnect();
        document.removeEventListener('visibilitychange', visibility);
        media.removeEventListener('change', motion);
        console.error('Supply renderer initialization failed', cause);
        handlers.current.onError?.(cause);
        setReadyScene(null);
        setError(true);
      });

    return () => {
      cancelled = true;
      if (frame) cancelAnimationFrame(frame);
      resize?.disconnect();
      document.removeEventListener('visibilitychange', visibility);
      media.removeEventListener('change', motion);
      if (scene.current === live) scene.current = null;
      setReadyScene(null);
      live?.dispose();
      live?.free();
    };
  }, [copyKey, attempt]);

  // Keep the scene in step with the dialog's phase. The reward's words reach
  // the scene only once the dialog has accepted a pick.
  useEffect(() => {
    const live = scene.current;
    if (!ready || !live) return;
    const kindIndex = KIND_INDEX[kind];
    const badge = kind.toUpperCase();
    if (phase === 'revealed' && !revealed.current && live.picked_index() < 0) {
      // A reward revealed earlier and not used yet: show it directly.
      revealed.current = true;
      live.show_revealed(kindIndex, name, value, detail, badge);
      return;
    }
    if (phase === 'opening' && !committed.current) {
      committed.current = true;
      live.open();
    }
    if (phase === 'choosing') live.skip_to_cards();
    if ((phase === 'revealing' || phase === 'revealed') && !revealed.current) {
      revealed.current = true;
      if (live.picked_index() < 0) live.pick(selected);
      live.reveal(kindIndex, name, value, detail, badge);
    }
  }, [phase, selected, kind, name, value, detail, readyScene, ready]);

  const point = (event: PointerEvent<HTMLCanvasElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return [event.clientX - bounds.left, event.clientY - bounds.top] as const;
  };
  /** Reports the opening; it must run inside the user's gesture so audio can unlock. */
  const commit = () => {
    const live = scene.current;
    if (!live || committed.current) return;
    committed.current = true;
    const [cue, readyAt] = Array.from(live.timing());
    handlers.current.onOpen(readyAt, cue);
  };

  return (
    <div className="pet-supply-rust">
      <canvas
        ref={canvas}
        className="pet-supply-canvas"
        aria-hidden="true"
        onPointerMove={(event) => scene.current?.pointer_move(...point(event))}
        onPointerDown={(event) => {
          if (!ready || event.button !== 0 || !event.isPrimary || activePointer.current !== null)
            return;
          if (scene.current?.pointer_down(...point(event))) {
            activePointer.current = event.pointerId;
            handlers.current.onGesture?.();
            event.currentTarget.setPointerCapture(event.pointerId);
          }
        }}
        onPointerUp={(event) => {
          const live = scene.current;
          if (
            !live ||
            !ready ||
            event.button !== 0 ||
            !event.isPrimary ||
            activePointer.current !== event.pointerId
          )
            return;
          activePointer.current = null;
          const result = live.pointer_up(...point(event));
          if (result === 1) live.open();
          if (result === 1 || result === 2) commit();
        }}
        onPointerCancel={(event) => {
          if (activePointer.current !== event.pointerId) return;
          activePointer.current = null;
          scene.current?.leave();
        }}
        onLostPointerCapture={(event) => {
          if (activePointer.current !== event.pointerId) return;
          activePointer.current = null;
          scene.current?.leave();
        }}
        onPointerLeave={() => {
          if (activePointer.current === null) scene.current?.leave();
        }}
        onContextMenu={(event) => event.preventDefault()}
        onDragStart={(event) => event.preventDefault()}
      />
      {!ready && !error && <span className="pet-supply-load">{t('pets.reward.loading')}</span>}
      {error && (
        <div className="pet-supply-load">
          <span>{t('pets.reward.renderError')}</span>
          <button
            type="button"
            onClick={() => {
              setError(false);
              setAttempt((n) => n + 1);
            }}
          >
            {t('aiCareer.retry')}
          </button>
        </div>
      )}
      {ready && phase === 'sealed' && (
        <button
          type="button"
          className="sr-only"
          onClick={() => {
            scene.current?.open();
            commit();
          }}
        >
          {t('pets.reward.open')}
        </button>
      )}
      {ready && phase === 'choosing' && (
        <div role="group" aria-label={t('pets.reward.choose')}>
          {[0, 1, 2, 3, 4].map((index) => (
            <button
              key={index}
              type="button"
              className="sr-only"
              onFocus={() => scene.current?.focus(index)}
              onBlur={() => scene.current?.focus(-1)}
              onClick={() => scene.current?.pick(index)}
            >
              {t('pets.reward.pick').replace('{number}', String(index + 1))}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
