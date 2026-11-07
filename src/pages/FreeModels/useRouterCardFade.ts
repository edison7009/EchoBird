import { useLayoutEffect, useRef } from 'react';

/** Crossfade only the details; keep the hub, arrows and numbered tabs alive. */
export function useRouterCardFade(routerId: string, enabled: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  const snapshot = useRef<HTMLElement | null>(null);
  const renderedContent = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    const panel = ref.current;
    const content = panel?.firstElementChild as HTMLElement | null;
    if (!panel || !content || !enabled) {
      snapshot.current = null;
      return;
    }
    const animations: Animation[] = [];
    let overlay: HTMLElement | null = null;
    if (snapshot.current && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      overlay = snapshot.current;
      overlay.classList.add('free-model-router-card-snapshot');
      overlay.setAttribute('aria-hidden', 'true');
      overlay.inert = true;
      overlay.querySelectorAll('[id]').forEach((node) => node.removeAttribute('id'));
      panel.append(overlay);
      const timing: KeyframeAnimationOptions = { duration: 180, easing: 'ease-out' };
      const outgoing = overlay.animate([{ opacity: 1 }, { opacity: 0 }], timing);
      const incoming = content.animate([{ opacity: 0 }, { opacity: 1 }], timing);
      animations.push(outgoing, incoming);
      outgoing.onfinish = () => overlay?.remove();
    }
    snapshot.current = null;
    return () => {
      // Sample the displayed blend before cancelling. Rapid changes continue
      // from what is visible instead of replaying an entrance from scratch.
      const image = document.createElement('div');
      const current = (renderedContent.current ?? content).cloneNode(true) as HTMLElement;
      current.style.opacity = getComputedStyle(content).opacity;
      image.append(current);
      if (overlay?.isConnected) {
        const previous = overlay.cloneNode(true) as HTMLElement;
        previous.style.opacity = getComputedStyle(overlay).opacity;
        image.append(previous);
      }
      snapshot.current = image;
      animations.forEach((animation) => animation.cancel());
      overlay?.remove();
    };
  }, [routerId, enabled]);

  useLayoutEffect(() => {
    renderedContent.current =
      (ref.current?.firstElementChild?.cloneNode(true) as HTMLElement) ?? null;
  });

  return ref;
}
