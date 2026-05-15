// Feedback page — guides users to capture failing logs and submit them
// as a GitHub issue. Two-step flow:
//   1. Open devtools (Console tab) so users can grab the failing log tail.
//   2. Open the project repo Issues page so they can paste + describe.
//
// Single Main component, no Panel — this page is full-width so users have
// room to read the instructions without distraction.

import { ExternalLink, Mail, Terminal } from 'lucide-react';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { useI18n } from '../../hooks/useI18n';
import { openDevtools } from '../../api/tauri';

const GITHUB_ISSUES_URL = 'https://github.com/edison7009/EchoBird/issues/new';
// Mainland China users frequently can't reach github.com — Gitcode mirror
// is the primary alternative; shown only under Chinese locale.
const GITCODE_ISSUES_URL = 'https://gitcode.com/edison7009/EchoBird/issues/create';
// English-locale fallback when GitHub is unreachable: direct email.
const SUPPORT_EMAIL = 'hi@echobird.ai';

const openExternal = (url: string) => shellOpen(url).catch(() => window.open(url, '_blank'));

export function FeedbackMain() {
  const { t, locale } = useI18n();
  const isZh = locale.startsWith('zh');

  return (
    <div className="max-w-2xl mx-auto py-8 px-2 space-y-8">
      <header className="space-y-3">
        <h1 className="cjk-title text-2xl">{t('feedback.title')}</h1>
        <p className="text-cyber-text-secondary leading-relaxed">{t('feedback.intro')}</p>
      </header>

      <section className="rounded-lg border border-cyber-border bg-cyber-bg-secondary/40 p-5 space-y-3">
        <div className="flex items-center gap-2">
          <Terminal size={18} className="text-cyber-accent" />
          <h2 className="font-semibold">{t('feedback.step1.title')}</h2>
        </div>
        <p className="text-sm text-cyber-text-secondary leading-relaxed">
          {t('feedback.step1.desc')}
        </p>
        <button
          onClick={() => {
            openDevtools().catch((e) => console.error('[Feedback] open_devtools failed', e));
          }}
          className="inline-flex items-center gap-2 px-4 py-2 rounded-md bg-cyber-accent/15 hover:bg-cyber-accent/25 border border-cyber-accent/40 text-cyber-accent transition-colors text-sm font-medium"
        >
          <Terminal size={14} />
          {t('feedback.step1.button')}
        </button>
      </section>

      <section className="rounded-lg border border-cyber-border bg-cyber-bg-secondary/40 p-5 space-y-3">
        <div className="flex items-center gap-2">
          <ExternalLink size={18} className="text-cyber-accent" />
          <h2 className="font-semibold">{t('feedback.step2.title')}</h2>
        </div>
        <p className="text-sm text-cyber-text-secondary leading-relaxed">
          {t('feedback.step2.desc')}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <button
            onClick={() => openExternal(GITHUB_ISSUES_URL)}
            className="inline-flex items-center gap-2 px-4 py-2 rounded-md bg-cyber-accent/15 hover:bg-cyber-accent/25 border border-cyber-accent/40 text-cyber-accent transition-colors text-sm font-medium"
          >
            <ExternalLink size={14} />
            {t('feedback.step2.button')}
          </button>
          {/* Locale-specific fallback channel:
              • zh users: Gitcode mirror (github.com is often unreachable from CN)
              • non-zh users: email (no widely-used GitHub mirror outside CN) */}
          {isZh ? (
            <button
              onClick={() => openExternal(GITCODE_ISSUES_URL)}
              className="inline-flex items-center gap-2 px-4 py-2 rounded-md bg-cyber-bg-secondary/60 hover:bg-cyber-bg-secondary border border-cyber-border text-cyber-text-secondary hover:text-cyber-text transition-colors text-sm font-medium"
            >
              <ExternalLink size={14} />
              {t('feedback.step2.fallbackButton')}
            </button>
          ) : (
            <button
              onClick={() => openExternal(`mailto:${SUPPORT_EMAIL}`)}
              className="inline-flex items-center gap-2 px-4 py-2 rounded-md bg-cyber-bg-secondary/60 hover:bg-cyber-bg-secondary border border-cyber-border text-cyber-text-secondary hover:text-cyber-text transition-colors text-sm font-medium"
            >
              <Mail size={14} />
              {SUPPORT_EMAIL}
            </button>
          )}
        </div>
        <p className="text-xs text-cyber-text-muted pt-1">{t('feedback.networkNote')}</p>
      </section>
    </div>
  );
}
