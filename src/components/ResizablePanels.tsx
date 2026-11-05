import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useI18n } from '../hooks/useI18n';
import './ResizablePanels.css';

const STORAGE_KEY = 'echobird-right-panel-width';
const DEFAULT_WIDTH = 300; // Existing w-80 at the app's 15px root font size.
const MIN_WIDTH = 260;
const MAX_WIDTH = 520;
const CENTER_MIN_WIDTH = 380;
const GAP = 6;

function readWidth() {
  try {
    const value = Number(localStorage.getItem(STORAGE_KEY));
    if (Number.isFinite(value) && value >= MIN_WIDTH && value <= MAX_WIDTH) return value;
  } catch {
    // Local preferences may be unavailable.
  }
  return DEFAULT_WIDTH;
}

export function ResizablePanels({ children }: { children: [ReactNode, ReactNode] }) {
  const { t } = useI18n();
  const root = useRef<HTMLDivElement>(null);
  const cleanup = useRef<(() => void) | null>(null);
  const [preferredWidth, setPreferredWidth] = useState(readWidth);
  const [available, setAvailable] = useState<number | null>(null);
  const [dragging, setDragging] = useState(false);
  const maximum =
    available === null
      ? MAX_WIDTH
      : Math.max(0, Math.min(MAX_WIDTH, available - GAP - CENTER_MIN_WIDTH));
  const minimum = Math.min(MIN_WIDTH, maximum);
  const width = Math.max(minimum, Math.min(maximum, preferredWidth));

  useLayoutEffect(() => {
    const element = root.current!;
    const measure = () => setAvailable(element.getBoundingClientRect().width);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => () => cleanup.current?.(), []);

  const saveWidth = (value: number) => {
    try {
      localStorage.setItem(STORAGE_KEY, String(value));
    } catch {
      // Keep resizing usable when local preferences cannot be saved.
    }
  };

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || cleanup.current) return;
    event.preventDefault();
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startWidth = width;
    const pointerId = event.pointerId;
    let current = startWidth;
    setDragging(true);
    const move = (next: PointerEvent) => {
      if (next.pointerId !== pointerId) return;
      const max = Math.max(
        0,
        Math.min(MAX_WIDTH, root.current!.getBoundingClientRect().width - GAP - CENTER_MIN_WIDTH)
      );
      current = Math.max(
        Math.min(MIN_WIDTH, max),
        Math.min(max, startWidth + startX - next.clientX)
      );
      setPreferredWidth(current);
    };
    const finish = (persist: boolean) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('blur', blur);
      if (target.hasPointerCapture(pointerId)) target.releasePointerCapture(pointerId);
      cleanup.current = null;
      if (persist) {
        setDragging(false);
        saveWidth(current);
      }
    };
    const end = (next: PointerEvent) => {
      if (next.pointerId === pointerId) finish(true);
    };
    const cancel = (next: PointerEvent) => {
      if (next.pointerId === pointerId) finish(true);
    };
    const blur = () => finish(true);
    cleanup.current = () => finish(false);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', cancel);
    window.addEventListener('blur', blur);
  };

  return (
    <div
      ref={root}
      className={`resizable-panels${dragging ? ' is-resizing' : ''}`}
      style={{ gridTemplateColumns: `minmax(0, 1fr) ${GAP}px ${width}px` }}
    >
      <div className="resizable-panels-center">{children[0]}</div>
      <div
        className="resizable-panels-handle"
        role="separator"
        aria-orientation="vertical"
        aria-label={t('nav.resizePanel')}
        aria-valuemin={minimum}
        aria-valuemax={maximum}
        aria-valuenow={width}
        tabIndex={0}
        onPointerDown={startResize}
        onDoubleClick={() => {
          setPreferredWidth(DEFAULT_WIDTH);
          saveWidth(DEFAULT_WIDTH);
        }}
        onKeyDown={(event) => {
          if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
          event.preventDefault();
          const value = Math.max(
            minimum,
            Math.min(maximum, width + (event.key === 'ArrowLeft' ? 16 : -16))
          );
          setPreferredWidth(value);
          saveWidth(value);
        }}
      >
        <span />
      </div>
      <div className="resizable-panels-right">{children[1]}</div>
    </div>
  );
}
