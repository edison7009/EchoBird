import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  ArrowUp,
  ChevronLeft,
  ChevronRight,
  LoaderCircle,
  Mic,
  Plus,
  Square,
  X,
} from 'lucide-react';
import { useI18n } from '../hooks/useI18n';
import { useNavigationStore } from '../stores/navigationStore';
import { useSidebarStore } from '../stores/sidebarStore';
import { VoiceAppIcon } from '../pages/VoiceInput/VoiceAppIcon';
import { VoiceFlow } from '../pages/VoiceInput/VoiceFlow';
import '../pages/VoiceInput/VoiceInput.css';
import './VoiceInputGuide.css';
import { ImportGuideDemo } from './ImportGuideDemo';

function VoiceGuideDemo() {
  const { t } = useI18n();
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => {
      if (!document.hidden) setElapsed((value) => (value + 100) % 8000);
    }, 100);
    return () => clearInterval(timer);
  }, []);
  const phase =
    elapsed < 900 ? 'idle' : elapsed < 4100 ? 'recording' : elapsed < 4700 ? 'processing' : 'done';
  return (
    <div className="voice-guide-desktop" data-phase={phase} aria-hidden="true">
      <div className="voice-guide-composer">
        <div className="voice-guide-window-dots">
          <i />
          <i />
          <i />
        </div>
        <div className="voice-guide-editor" data-filled={phase === 'done'}>
          <span className="voice-guide-placeholder">{t('voice.guide.placeholder')}</span>
          <span className="voice-guide-example">{t('voice.guide.example')}</span>
        </div>
        <div className="voice-guide-tools">
          <Plus size={17} />
          <span>
            <ArrowUp size={16} />
          </span>
        </div>
      </div>
      <div className="voice-guide-island">
        <div className="voice-island voice-theme-dark" data-active="true">
          <VoiceAppIcon src="/brand/bird.png" loading={false} />
          <div className="voice-island-middle">
            <VoiceFlow
              active
              speaking={phase === 'recording'}
              level={phase === 'recording' ? 0.6 + Math.sin(elapsed / 170) * 0.28 : 0}
              palette="sunset"
              theme="dark"
            />
          </div>
          <span className="voice-island-control voice-island-confirm">
            {phase === 'recording' ? (
              <Square size={13} fill="currentColor" />
            ) : phase === 'processing' ? (
              <LoaderCircle size={16} className="motion-safe:animate-spin" />
            ) : (
              <Mic size={16} />
            )}
          </span>
          <i className="voice-guide-tap" />
        </div>
      </div>
    </div>
  );
}

export function VoiceInputGuide({ onClose }: { onClose: () => void }) {
  const { t } = useI18n();
  const [page, setPage] = useState(0);
  const isVoice = page === 1;
  const collapsed = useSidebarStore((state) => state.collapsed);
  const root = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const [position, setPosition] = useState<{
    left: number;
    top: number;
    width: number;
    arrow: number;
    anchor: { left: number; top: number; width: number; height: number } | null;
  } | null>(null);
  const openVoice = () => {
    useNavigationStore.getState().setActivePage('voice');
    onClose();
  };
  const positioned = position !== null;
  useLayoutEffect(() => {
    const anchor = isVoice ? document.getElementById('voice-input-nav') : null;
    anchor?.scrollIntoView({ block: 'nearest' });
    const place = () => {
      const rect = anchor?.getBoundingClientRect();
      const width = Math.min(
        420,
        window.innerWidth - 32,
        Math.max(220, window.innerWidth - (rect?.right ?? 0) - 32)
      );
      const height = panel.current?.getBoundingClientRect().height ?? 380;
      const center = rect ? rect.top + rect.height / 2 : window.innerHeight / 2;
      const top = Math.max(16, Math.min(center - height / 2, window.innerHeight - height - 16));
      setPosition({
        left: rect
          ? Math.max(16, Math.min(rect.right + 18, window.innerWidth - width - 16))
          : Math.max(16, (window.innerWidth - width) / 2),
        top,
        width,
        arrow: Math.max(16, Math.min(center - top, height - 16)),
        anchor: rect
          ? {
              left: rect.left - 4,
              top: rect.top - 4,
              width: rect.width + 8,
              height: rect.height + 8,
            }
          : null,
      });
    };
    place();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(place);
    if (anchor) observer?.observe(anchor);
    if (panel.current) observer?.observe(panel.current);
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  }, [collapsed, isVoice]);
  useEffect(() => {
    if (!positioned) return;
    const previous = document.activeElement as HTMLElement | null;
    const dialog = root.current;
    close.current?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }
      if (event.key !== 'Tab') return;
      const buttons = dialog?.querySelectorAll<HTMLButtonElement>('button');
      const first = buttons?.[0];
      const last = buttons?.[buttons.length - 1];
      if (!dialog?.contains(document.activeElement)) {
        event.preventDefault();
        first?.focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    window.addEventListener('keydown', keydown);
    return () => {
      window.removeEventListener('keydown', keydown);
      if (dialog?.contains(document.activeElement) || document.activeElement === document.body) {
        if (previous?.isConnected && previous.getClientRects().length) previous.focus();
        else document.getElementById('voice-input-nav')?.focus();
      }
    };
  }, [onClose, positioned]);
  return (
    <div
      ref={root}
      className="voice-guide-root"
      role="dialog"
      aria-modal="true"
      aria-label={t(isVoice ? 'voice.input' : 'settings.autoImport')}
      aria-describedby="voice-guide-description"
    >
      <div className="voice-guide-backdrop" onClick={onClose} />
      {position?.anchor && (
        <button
          type="button"
          className="voice-guide-target"
          style={position.anchor}
          aria-label={t('voice.input')}
          onClick={openVoice}
        />
      )}
      <aside
        className="voice-guide-position"
        style={
          position
            ? { left: position.left, top: position.top, width: position.width }
            : { visibility: 'hidden' }
        }
      >
        {position?.anchor && <i className="voice-guide-arrow" style={{ top: position.arrow }} />}
        <section ref={panel} className="voice-guide-panel page-scroll">
          <header className="voice-guide-header">
            <h2 className="text-base font-bold">{t('guide.title')}</h2>
            <button
              ref={close}
              type="button"
              className="voice-guide-close close-button"
              onClick={onClose}
              aria-label={t('btn.close')}
            >
              <X size={17} />
            </button>
          </header>
          {isVoice ? <VoiceGuideDemo /> : <ImportGuideDemo />}
          <p id="voice-guide-description">
            {t(isVoice ? 'voice.guide.description' : 'guide.importDescription')}
          </p>
          <nav className="voice-guide-pagination" aria-label={t('guide.pages')}>
            <div className="voice-guide-pagination-controls">
              <button
                type="button"
                aria-label={t('guide.previous')}
                aria-disabled={page === 0}
                onClick={() => setPage(0)}
              >
                <ChevronLeft size={18} />
              </button>
              <span aria-live="polite">{`${page + 1}/2`}</span>
              <button
                type="button"
                aria-label={t('guide.next')}
                aria-disabled={page === 1}
                onClick={() => setPage(1)}
              >
                <ChevronRight size={18} />
              </button>
            </div>
            <button type="button" className="voice-guide-skip" onClick={onClose}>
              {t('guide.skip')}
            </button>
          </nav>
        </section>
      </aside>
    </div>
  );
}
