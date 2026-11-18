// ChangelogDialog — "更新日志 / Changelog" modal. Opened from the title-bar
// button (left of Feedback) and auto-opened once on the first launch after an
// update. Content is the current release's notes read straight from the
// version manifest (echobird.ai/api/version/index.json); only the section
// matching the active UI locale is shown.
import React, { useState, useEffect, useCallback, useRef } from 'react';
import { X } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useI18n } from '../hooks/useI18n';
import { mdComponents } from '../pages/MotherAgent/mdComponents';
import { VoiceInputGuide } from './VoiceInputGuide';
import { fetchLatestManifest, type UpdateManifest } from '../services/updateManifest';

interface Manifest extends UpdateManifest {
  // Legacy single-blob notes ("## 中文 / ## English / ## 日本語"), kept for
  // manifests written before the per-language split.
  releaseNotes?: string;
  // Preferred: notes stored per language, keyed by a short language code
  // ("en" / "zh-cn" / "zh-tw" / "ja") — the marker the manifest author writes.
  // The app maps its locale to the matching code (see LOCALE_TO_NOTES_CODE).
  releaseNotesI18n?: Record<string, string>;
}

// Map an app locale to the manifest's language marker. The manifest uses
// friendly codes (zh-cn / zh-tw) rather than the app's internal locale ids
// (zh-Hans / zh-Hant), so hand-writing the notes JSON reads naturally.
const LOCALE_TO_NOTES_CODE: Record<string, string> = {
  en: 'en',
  'zh-Hans': 'zh-cn',
  'zh-Hant': 'zh-tw',
  ja: 'ja',
};

// releaseNotes is markdown with per-language sections ("## 中文 / ## English /
// ## 日本語"). Return just the section for the active locale, header stripped,
// so a user sees only their language. Falls back to the whole string when no
// section matches (e.g. a future single-language note).
function sectionForLocale(releaseNotes: string, locale: string): string {
  const label = locale === 'en' ? 'English' : locale === 'ja' ? '日本語' : '中文';
  const blocks = releaseNotes
    .split(/\n?##\s+/)
    .map((b) => b.trim())
    .filter(Boolean);
  for (const block of blocks) {
    const nl = block.indexOf('\n');
    const head = (nl === -1 ? block : block.slice(0, nl)).trim();
    if (head === label) {
      return nl === -1 ? '' : block.slice(nl + 1).trim();
    }
  }
  return releaseNotes.trim();
}

// Pick the notes for the active locale: prefer the structured per-language
// field, falling back to English, then to any language present, then to
// parsing the legacy single-blob releaseNotes.
function notesForLocale(m: Manifest, locale: string): string {
  const i18n = m.releaseNotesI18n;
  if (i18n && typeof i18n === 'object') {
    const code = LOCALE_TO_NOTES_CODE[locale] ?? locale;
    const picked = i18n[code] || i18n.en || i18n['zh-cn'] || Object.values(i18n)[0];
    if (picked) return picked.trim();
  }
  if (m.releaseNotes) return sectionForLocale(m.releaseNotes, locale);
  return '';
}

// The shared Mother-Agent markdown components (mdComponents) are tuned for
// compact chat bubbles — they define no paragraph margins (Tailwind's preflight
// zeroes them) and use tight list spacing. In a changelog that reads as
// everything crammed together, so we spread the base set and override the block
// elements with comfortable reading room. `first:mt-0 last:mb-0` keeps the gaps
// between blocks without padding the very top/bottom of the scroll area.
const changelogMdComponents = {
  ...mdComponents,
  p: ({ children }: { children?: React.ReactNode }) => (
    <p className="my-3 leading-relaxed first:mt-0 last:mb-0">{children}</p>
  ),
  ul: ({ children }: { children?: React.ReactNode }) => (
    <ul className="list-disc list-outside pl-5 my-3 space-y-1.5 first:mt-0 last:mb-0">
      {children}
    </ul>
  ),
  ol: ({ children }: { children?: React.ReactNode }) => (
    <ol className="list-decimal list-outside pl-5 my-3 space-y-1.5 first:mt-0 last:mb-0">
      {children}
    </ol>
  ),
  li: ({ children }: { children?: React.ReactNode }) => (
    <li className="leading-relaxed">{children}</li>
  ),
  h1: ({ children }: { children?: React.ReactNode }) => (
    <h1 className="text-lg font-bold text-cyber-text mt-5 mb-2 first:mt-0">{children}</h1>
  ),
  h2: ({ children }: { children?: React.ReactNode }) => (
    <h2 className="text-base font-bold text-cyber-text mt-5 mb-2 first:mt-0">{children}</h2>
  ),
  h3: ({ children }: { children?: React.ReactNode }) => (
    <h3 className="text-sm font-bold text-cyber-text mt-4 mb-1.5 first:mt-0">{children}</h3>
  ),
};

interface ChangelogDialogProps {
  isOpen: boolean;
  onClose: () => void;
  showVoiceGuideOnClose?: boolean;
}

export const ChangelogDialog: React.FC<ChangelogDialogProps> = ({
  isOpen,
  onClose,
  showVoiceGuideOnClose = false,
}) => {
  const { t, locale } = useI18n();
  const [isAnimatingOut, setIsAnimatingOut] = useState(false);
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  // Whether we've ever loaded good notes. Once true, a later failed refetch
  // keeps the last content on screen instead of flipping to the error state.
  const loadedRef = useRef(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [showGuide, setShowGuide] = useState(false);
  const closeGuide = useCallback(() => setShowGuide(false), []);
  useEffect(
    () => () => {
      if (closeTimer.current !== null) clearTimeout(closeTimer.current);
    },
    []
  );

  // Fetch the manifest each time the dialog opens — it's a tiny JSON and we
  // want the freshest notes. The initial state is 'loading' (covers the first
  // open); on later opens we keep the last content visible and refetch
  // silently, so there's no loading flash. Failure surfaces as the error state.
  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    void (async () => {
      try {
        const data = (await fetchLatestManifest()) as Manifest | null;
        if (!data) throw new Error('manifest unavailable');
        if (cancelled) return;
        setManifest(data);
        setStatus('ready');
        loadedRef.current = true;
      } catch {
        // Keep the last-good notes visible on a silent refetch failure; only
        // show the error screen when there's nothing loaded yet.
        if (!cancelled && !loadedRef.current) setStatus('error');
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [isOpen]);

  const handleClose = useCallback(() => {
    if (closeTimer.current !== null) return;
    setIsAnimatingOut(true);
    closeTimer.current = setTimeout(() => {
      closeTimer.current = null;
      setIsAnimatingOut(false);
      setShowGuide(showVoiceGuideOnClose);
      onClose();
    }, 200);
  }, [onClose, showVoiceGuideOnClose]);

  // ESC to close
  useEffect(() => {
    if (!isOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') handleClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [isOpen, handleClose]);

  if (!isOpen) return showGuide ? <VoiceInputGuide onClose={closeGuide} /> : null;

  const notes = manifest ? notesForLocale(manifest, locale) : '';

  return (
    <div
      className={`fixed inset-0 z-[9998] flex items-center justify-center transition-all duration-200 ${
        isAnimatingOut ? 'opacity-0' : 'opacity-100'
      }`}
    >
      {/* Backdrop */}
      <div className="absolute inset-0" onClick={handleClose} />

      {/* Dialog */}
      <div
        className={`relative flex max-h-[80vh] w-[clamp(560px,56vw,860px)] max-w-[92vw] flex-col border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden transition-[opacity,transform] duration-200 ${
          isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top accent line */}
        <div className="h-px w-full flex-shrink-0 bg-cyber-border" />

        {/* Header */}
        <div className="px-6 pt-5 pb-4 flex flex-shrink-0 items-center justify-between">
          <div className="flex items-baseline gap-2 min-w-0">
            <span className="text-lg font-bold text-cyber-text">{t('nav.changelog')}</span>
            {manifest?.version && (
              <span className="text-xs font-mono text-cyber-accent">v{manifest.version}</span>
            )}
            {manifest?.releaseDate && (
              <span className="text-xs font-mono text-cyber-text-secondary">
                {manifest.releaseDate}
              </span>
            )}
          </div>
          <button
            onClick={handleClose}
            className="text-cyber-text-secondary close-button transition-colors flex-shrink-0"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="min-h-0 px-6 pb-6 overflow-y-auto">
          {status === 'loading' && (
            <p className="text-sm text-cyber-text-secondary py-8 text-center">
              {t('changelog.loading')}
            </p>
          )}
          {status === 'error' && (
            <p className="text-sm text-cyber-text-secondary py-8 text-center">
              {t('changelog.error')}
            </p>
          )}
          {status === 'ready' &&
            (notes ? (
              <div className="text-sm leading-relaxed text-cyber-text">
                <ReactMarkdown remarkPlugins={[remarkGfm]} components={changelogMdComponents}>
                  {notes}
                </ReactMarkdown>
              </div>
            ) : (
              <p className="text-sm text-cyber-text-secondary py-8 text-center">
                {t('changelog.empty')}
              </p>
            ))}
        </div>
      </div>
    </div>
  );
};
