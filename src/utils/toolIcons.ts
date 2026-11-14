import type { LocalTool } from '../api/types';

// Retain decoded images across refreshes and tab remounts, including fallback choices.
const images = new Map<string, Promise<HTMLImageElement | null>>();
const decodedImages = new Map<string, HTMLImageElement>();
const failedImages = new Set<string>();

export const getDecodedImage = (src: string) => decodedImages.get(src);
export const rememberDecodedImage = (src: string, image: HTMLImageElement) => {
  failedImages.delete(src);
  decodedImages.set(src, image);
};

export const isToolIconReady = (src?: string) =>
  !src || typeof Image === 'undefined' || decodedImages.has(src) || failedImages.has(src);

export const resolvedToolIcon = (src?: string) => (src && !failedImages.has(src) ? src : '');

export function preloadToolImage(src: string): Promise<HTMLImageElement | null> {
  const decoded = decodedImages.get(src);
  if (decoded) return Promise.resolve(decoded);
  let pending = images.get(src);
  if (!pending) {
    const image = new Image();
    image.src = src;
    pending = new Promise((resolve) => {
      let settled = false;
      const finish = (loaded: HTMLImageElement | null) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        if (loaded) rememberDecodedImage(src, loaded);
        else failedImages.add(src);
        resolve(loaded);
      };
      // A broken or unreachable plugin image must not hold the desktop forever.
      const timer = setTimeout(() => {
        finish(null);
        image.src = '';
      }, 3000);
      void image.decode().then(
        () => finish(image),
        () => finish(null)
      );
    });
    images.set(src, pending);
  }
  return pending;
}

/** Prepare the actual sources selected by cache, detection or plugin overrides. */
export function preloadDesktopToolIcons(tools: LocalTool[]): Promise<LocalTool[]> {
  return Promise.all(
    tools.map(async (tool) => {
      if (!tool.icon || typeof Image === 'undefined') return tool;
      const image = await preloadToolImage(tool.icon);
      return image ? tool : { ...tool, icon: '' };
    })
  );
}

export function preloadToolIcons(tools: LocalTool[]): Promise<LocalTool[]> {
  return Promise.all(
    tools.map(async (tool) => {
      const sources = [
        ...(['dsh', 'musecode', 'opendesign'].includes(tool.id)
          ? []
          : [`./icons/tools/${tool.id}.svg`]),
        `./icons/tools/${tool.id}.png`,
        ...(tool.iconBase64 ? [tool.iconBase64] : []),
      ];
      for (const src of sources) {
        const image = await preloadToolImage(src);
        if (image) return { ...tool, icon: src };
      }
      return { ...tool, icon: '' };
    })
  );
}
