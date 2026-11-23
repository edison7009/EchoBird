// Release notes shared by the Settings changelog and the pre-install dialog.
import React, { useState, useEffect, useCallback, useRef } from 'react';
import { X } from 'lucide-react';
import { openExternal } from '../api/tauri';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useI18n } from '../hooks/useI18n';
import { mdComponents } from '../pages/MotherAgent/mdComponents';
import { VoiceInputGuide } from './VoiceInputGuide';
import { PageActionButton } from './PageActionButton';
import {
  fetchLatestManifest,
  fetchManifestForVersion,
  type UpdateManifest,
} from '../services/updateManifest';

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
function notesForLocale(m: UpdateManifest, locale: string): string {
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

const GITHUB_ISSUES_URL = 'https://github.com/edison7009/EchoBird/issues';
const GITCODE_ISSUES_URL = 'https://gitcode.com/edison7009/EchoBird/issues/';

const feedbackLinkClass =
  'text-sm text-cyber-text-secondary underline-offset-4 hover:text-cyber-text hover:underline focus-visible:underline';

function openFeedbackLink(event: React.MouseEvent<HTMLAnchorElement>) {
  event.preventDefault();
  const href = event.currentTarget.href;
  void openExternal(href).catch(() => window.open(href, '_blank', 'noopener,noreferrer'));
}

interface ChangelogDialogProps {
  isOpen: boolean;
  onClose: () => void;
  showVoiceGuideOnClose?: boolean;
  update?: {
    fromVersion: string;
    toVersion: string;
    manifest?: UpdateManifest;
    starting: boolean;
    onStart: () => void;
  };
}

export const ChangelogDialog: React.FC<ChangelogDialogProps> = ({
  isOpen,
  onClose,
  showVoiceGuideOnClose = false,
  update,
}) => {
  const { t, locale } = useI18n();
  const [isAnimatingOut, setIsAnimatingOut] = useState(false);
  const [manifest, setManifest] = useState<UpdateManifest | null>(null);
  const targetVersion = update?.toVersion;
  const suppliedManifest = update?.manifest;
  const starting = update?.starting ?? false;
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  // Whether we've ever loaded good notes. Once true, a later failed refetch
  // keeps the last content on screen instead of flipping to the error state.
  const loadedRef = useRef(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
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
    if (!isOpen || suppliedManifest) return;
    const controller = new AbortController();
    const { signal } = controller;
    void (async () => {
      try {
        const data = targetVersion
          ? await fetchManifestForVersion(targetVersion, signal)
          : await fetchLatestManifest(signal);
        if (!data) throw new Error('manifest unavailable');
        if (signal.aborted) return;
        setManifest(data);
        setStatus('ready');
        loadedRef.current = true;
      } catch {
        // Keep the last-good notes visible on a silent refetch failure; only
        // show the error screen when there's nothing loaded yet.
        if (!signal.aborted && !loadedRef.current) setStatus('error');
      }
    })();
    return () => controller.abort();
  }, [isOpen, suppliedManifest, targetVersion]);

  const handleClose = useCallback(() => {
    if (starting || closeTimer.current !== null) return;
    setIsAnimatingOut(true);
    closeTimer.current = setTimeout(() => {
      closeTimer.current = null;
      setIsAnimatingOut(false);
      setShowGuide(showVoiceGuideOnClose);
      onClose();
    }, 200);
  }, [onClose, showVoiceGuideOnClose, starting]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!isOpen || !dialog) return;
    const doc = dialog.ownerDocument;
    const previous = doc.activeElement;
    dialog.focus({ preventScroll: true });
    return () => {
      if (
        previous instanceof HTMLElement &&
        previous.isConnected &&
        (dialog.contains(doc.activeElement) || doc.activeElement === doc.body)
      ) {
        previous.focus({ preventScroll: true });
      }
    };
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        handleClose();
        return;
      }
      const dialog = dialogRef.current;
      if (event.key !== 'Tab' || !dialog) return;
      const controls = dialog.querySelectorAll<HTMLElement>('button:not(:disabled), a[href]');
      const first = controls[0];
      const last = controls[controls.length - 1];
      const focused = dialog.ownerDocument.activeElement;
      const boundary = event.shiftKey ? first : last;
      if (focused === boundary || focused === dialog || !dialog.contains(focused)) {
        event.preventDefault();
        (event.shiftKey ? last : first)?.focus();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [isOpen, handleClose]);

  if (!isOpen) return showGuide ? <VoiceInputGuide onClose={closeGuide} /> : null;

  const displayedManifest = suppliedManifest ?? manifest;
  const displayedStatus = suppliedManifest ? 'ready' : status;
  const notes = displayedManifest ? notesForLocale(displayedManifest, locale) : '';
  const title = update
    ? t('changelog.updateTitle')
        .replace('{from}', 'v' + update.fromVersion)
        .replace('{to}', 'v' + update.toVersion)
    : t('nav.changelog');

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
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        aria-label={title}
        className={`relative flex max-h-[80vh] w-[clamp(560px,56vw,860px)] max-w-[92vw] flex-col border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden outline-none transition-[opacity,transform] duration-200 ${
          isAnimatingOut ? 'scale-95 opacity-0' : 'scale-100 opacity-100'
        }`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Top accent line */}
        <div className="h-px w-full flex-shrink-0 bg-cyber-border" />

        {/* Header */}
        <div className="px-6 pt-5 pb-4 flex flex-shrink-0 items-center justify-between gap-3">
          <div className="flex items-baseline gap-2 min-w-0">
            <span className="text-lg font-bold text-cyber-text">{title}</span>
            {!update && manifest?.version && (
              <span className="text-xs font-mono text-cyber-accent">v{manifest.version}</span>
            )}
            {!update && manifest?.releaseDate && (
              <span className="text-xs font-mono text-cyber-text-secondary">
                {manifest.releaseDate}
              </span>
            )}
          </div>
          <button
            type="button"
            aria-label={t('btn.close')}
            disabled={starting}
            onClick={handleClose}
            className="text-cyber-text-secondary close-button transition-colors flex-shrink-0"
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="min-h-0 px-6 pb-6 overflow-y-auto">
          {displayedStatus === 'loading' && (
            <p className="text-sm text-cyber-text-secondary py-8 text-center">
              {t('changelog.loading')}
            </p>
          )}
          {displayedStatus === 'error' && (
            <p className="text-sm text-cyber-text-secondary py-8 text-center">
              {t('changelog.error')}
            </p>
          )}
          {displayedStatus === 'ready' &&
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

        <footer className="flex shrink-0 flex-wrap items-center justify-end gap-x-3 gap-y-2 border-t border-cyber-border/30 px-6 py-3">
          <a href={GITHUB_ISSUES_URL} className={feedbackLinkClass} onClick={openFeedbackLink}>
            {t('nav.feedback')}
          </a>
          {locale === 'zh-Hans' && (
            <a href={GITCODE_ISSUES_URL} className={feedbackLinkClass} onClick={openFeedbackLink}>
              国内反馈
            </a>
          )}
          <PageActionButton
            onClick={update ? update.onStart : handleClose}
            disabled={isAnimatingOut}
            pending={starting}
            className="is-key px-4 font-semibold"
          >
            {t(update ? 'changelog.startUpdate' : 'changelog.gotIt')}
          </PageActionButton>
        </footer>
      </div>
    </div>
  );
};
