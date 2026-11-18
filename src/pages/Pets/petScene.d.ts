import type { PetModel } from './petConstants';

export interface PetScene {
  show(pet: PetModel): void;
  /**
   * Plays the feedback for `next`: absorbing growth (same level), a level, the
   * level-40 milestone, a form change (then a short landing) or level 60.
   * `done` fires once when everything visible has finished; `cues.onImpact`
   * fires once at the visual peak (no peak when only absorbing). A `show` or
   * `dispose` cancels both.
   */
  upgrade(
    next: PetModel,
    done: () => void,
    onProgress?: (progress: number) => void,
    cues?: { onImpact?: () => void }
  ): void;
  interact(): void;
  rotate(dx: number, dy: number): void;
  thumbnail(pet: PetModel): string;
  dispose(): void;
}
/** Loads the WebAssembly renderer; resolve it before calling createPetScene. */
export function loadPetScene(load?: () => Promise<unknown>): Promise<void>;
/** `options.reduced` overrides the system's reduced-motion setting. */
export function createPetScene(
  canvas: HTMLCanvasElement,
  options?: { reduced?: boolean }
): PetScene;
