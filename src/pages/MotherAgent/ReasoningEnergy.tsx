import { useEffect, useRef, useState } from 'react';
import { useDocumentVisible } from '../../hooks/useDocumentVisible';
import {
  createReasoningEnergyRenderer,
  type ReasoningEnergyRenderer,
} from './reasoningEnergyRenderer';

export function ReasoningEnergy({ active }: { active: boolean }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rendererRef = useRef<ReasoningEnergyRenderer | null>(null);
  const activeRef = useRef(active);
  const visible = useDocumentVisible();
  const [started, setStarted] = useState(active);
  const [ready, setReady] = useState<boolean | null>(null);
  if (active && !started) setStarted(true);

  useEffect(() => {
    activeRef.current = active;
    rendererRef.current?.setActive(active);
  }, [active]);

  useEffect(() => {
    const host = hostRef.current;
    if (!started || !visible || !host) return;
    const preferences = [
      '(prefers-reduced-motion: reduce)',
      '(prefers-contrast: more)',
      '(forced-colors: active)',
    ].map((query) => window.matchMedia(query));
    const destroy = () => {
      const canvas = canvasRef.current;
      canvasRef.current = null;
      const renderer = rendererRef.current;
      rendererRef.current = null;
      canvas?.removeEventListener('webglcontextlost', lost);
      canvas?.remove();
      renderer?.dispose();
      setReady(false);
    };
    const lost = (event: Event) => {
      event.preventDefault();
      destroy();
    };
    const update = () => {
      if (preferences.some((preference) => preference.matches)) {
        destroy();
        return;
      }
      if (rendererRef.current) return;
      const canvas = document.createElement('canvas');
      canvas.className = 'mother-reasoning-energy-canvas';
      host.appendChild(canvas);
      const renderer = createReasoningEnergyRenderer(canvas);
      if (!renderer) {
        canvas.remove();
        canvas.getContext('webgl2')?.getExtension('WEBGL_lose_context')?.loseContext();
        setReady(false);
        return;
      }
      canvasRef.current = canvas;
      rendererRef.current = renderer;
      canvas.addEventListener('webglcontextlost', lost);
      renderer.setActive(activeRef.current);
      setReady(true);
    };
    preferences.forEach((preference) => preference.addEventListener('change', update));
    update();
    return () => {
      preferences.forEach((preference) => preference.removeEventListener('change', update));
      destroy();
    };
  }, [started, visible]);

  return (
    <div
      aria-hidden="true"
      className="mother-reasoning-energy"
      data-active={(active && visible && ready === true) || undefined}
      data-unavailable={ready === false || undefined}
    >
      <div className="mother-reasoning-energy-base" />
      <div ref={hostRef} className="mother-reasoning-energy-field" />
    </div>
  );
}
