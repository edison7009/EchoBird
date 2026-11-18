import type { PetModel } from './petConstants';

export interface PetScene {
  show(pet: PetModel): void;
  upgrade(next: PetModel, done: () => void, onProgress?: (progress: number) => void): void;
  interact(): void;
  rotate(dx: number, dy: number): void;
  thumbnail(pet: PetModel): string;
  dispose(): void;
}
/** Loads the WebAssembly renderer; resolve it before calling createPetScene. */
export function loadPetScene(load?: () => Promise<unknown>): Promise<void>;
export function createPetScene(canvas: HTMLCanvasElement): PetScene;
