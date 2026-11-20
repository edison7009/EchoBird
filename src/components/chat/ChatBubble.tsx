// ChatBubble — modern AI conversation style.
// Assistant: full-width markdown, no bubble (plain text on the page bg).
// User: subtle right-aligned card. Streaming caret while the agent types.
import { memo, useEffect, useState } from 'react';
import { Check, CircleAlert, Copy, Paperclip, KeyRound, Image as ImageIcon } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { getModelIcon } from '../cards/ModelCard';
import { useI18n } from '../../hooks/useI18n';
import { mdComponents } from '../../pages/MotherAgent/mdComponents';
import { IS_LINUX } from '../../utils/platform';
import { copyText } from '../../utils/copyText';
import type { AssistantReplySummary } from '../../api/types';
import workingMark from '../../assets/working-mark.webp';

export type BubbleRole = 'user' | 'assistant' | 'system' | 'error' | 'working' | 'skeleton';

export interface BubbleChip {
  type: 'file' | 'image' | 'model';
  name: string;
  /** For model chips: model provider name to look up icon */
  modelId?: string;
  /** For image chips: base64 preview data URL */
  preview?: string;
}

export interface ChatBubbleProps {
  role: BubbleRole;
  content: string;
  chips?: BubbleChip[];
  isStreaming?: boolean;
  subContent?: string;
  replySummary?: AssistantReplySummary | null;
  workingStartedAt?: number;
}

// ── Icon-only readonly chips below user message ───────────────────────────────
const BASE_CHIP = 'flex items-center justify-center w-6 h-6 rounded border flex-shrink-0';
const CHIP_MODEL = `${BASE_CHIP} bg-cyber-text/10 border-cyber-border/40`;
const CHIP_FILE = `${BASE_CHIP} bg-cyber-bg/60 border-cyber-text-muted/30`;

function ReadonlyChips({ chips }: { chips: BubbleChip[] }) {
  return (
    <div className="flex flex-wrap gap-1 mt-1.5">
      {chips.map((c, i) => {
        if (c.type === 'model') {
          const icon = getModelIcon('', c.modelId || '');
          return (
            <span key={i} className={CHIP_MODEL}>
              {icon ? (
                <img src={icon} alt="" className="w-4 h-4" />
              ) : (
                <KeyRound size={12} className="text-cyber-accent" />
              )}
            </span>
          );
        }
        if (c.type === 'image') {
          return (
            <span key={i} className={CHIP_FILE}>
              {c.preview ? (
                <img src={c.preview} alt={c.name} className="w-4 h-4 object-cover rounded" />
              ) : (
                <ImageIcon size={12} className="text-cyber-text-muted" />
              )}
            </span>
          );
        }
        return (
          <span key={i} className={CHIP_FILE}>
            <Paperclip size={12} className="text-cyber-text-muted" />
          </span>
        );
      })}
    </div>
  );
}

function TurnElapsed({ startedAt }: { startedAt: number }) {
  const { t, locale } = useI18n();
  const [seconds, setSeconds] = useState(() =>
    Math.max(0, Math.floor((performance.now() - startedAt) / 1000))
  );
  useEffect(() => {
    const timer = window.setInterval(
      () => setSeconds(Math.max(0, Math.floor((performance.now() - startedAt) / 1000))),
      250
    );
    return () => window.clearInterval(timer);
  }, [startedAt]);
  const format = (value: number, unit: 'minute' | 'second') =>
    new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay: 'short' }).format(value);
  const remainder = format(seconds % 60, 'second');
  const duration =
    seconds < 60 ? remainder : `${format(Math.floor(seconds / 60), 'minute')} ${remainder}`;
  return (
    <>
      {' '}
      <span aria-hidden="true">·</span>{' '}
      <time dateTime={`PT${seconds}S`}>
        {t('mother.waitElapsed').replace('{duration}', duration)}
      </time>{' '}
      <span aria-hidden="true">···</span>
    </>
  );
}

function WorkingStatus({ startedAt }: { startedAt?: number }) {
  const { t } = useI18n();
  return (
    <div className="my-4 flex min-w-0 items-center gap-2 text-sm">
      {IS_LINUX ? (
        <span
          className="w-[18px] shrink-0 text-center font-mono text-cyber-accent"
          aria-hidden="true"
        >
          ⠋
        </span>
      ) : (
        <span className="spinner-mark" aria-hidden="true">
          <img src={workingMark} alt="" />
        </span>
      )}
      <span className="spinner-shimmer whitespace-nowrap tabular-nums">
        {t('mother.waitThinking')}
        {startedAt !== undefined && <TurnElapsed key={startedAt} startedAt={startedAt} />}
      </span>
    </div>
  );
}

// ── Content size guard (markdown blow-up protection) ──────────────────────────
const CONTENT_LIMIT = 12000;
const truncate = (text: string) =>
  text.length > CONTENT_LIMIT ? text.slice(0, CONTENT_LIMIT) + '…' : text;

// Strip the agent's internal scaffolding while keeping the user-facing markdown.
// `<think>...</think>` is private reasoning. `<chat>...</chat>` is a wrapper the
// system prompt asks the agent to emit; we keep the inside but drop the tags.
function cleanAgentText(content: string): string {
  return content
    .replace(/<think>[\s\S]*?<\/think>/gi, '')
    .replace(/<think>[\s\S]*$/gi, '')
    .replace(/<\/?chat[^>]*>/gi, '')
    .trim();
}

function ReplyFooter({ text, summary }: { text: string; summary: AssistantReplySummary | null }) {
  const { t, locale } = useI18n();
  const [copyFeedback, setCopyFeedback] = useState<{ result: 'copied' | 'failed' }>();
  useEffect(() => {
    if (!copyFeedback) return;
    const timer = window.setTimeout(() => setCopyFeedback(undefined), 2000);
    return () => window.clearTimeout(timer);
  }, [copyFeedback]);

  const compactNumber = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
  const metrics: string[] = [];
  if (summary) {
    const seconds = summary.durationMs / 1000;
    metrics.push(
      t('mother.replyDuration').replace(
        '{seconds}',
        compactNumber.format(seconds >= 1 ? Math.round(seconds) : seconds)
      )
    );
    if (summary.totalTokens !== undefined) {
      const tokens = summary.totalTokens;
      const [scale, suffix]: [number, string] =
        tokens >= 1e9
          ? [1e9, 'b']
          : tokens >= 1e6
            ? [1e6, 'm']
            : tokens >= 1e3
              ? [1e3, 'k']
              : [1, ''];
      metrics.push(
        t('mother.replyTokens').replace('{count}', compactNumber.format(tokens / scale) + suffix)
      );
    }
    if (summary.estimatedCostMicros !== undefined) {
      const cost = summary.estimatedCostMicros / 1_000_000;
      metrics.push(`~$${cost.toFixed(cost === 0 || cost >= 0.01 ? 2 : cost < 0.0001 ? 6 : 4)}`);
    }
  }
  const copyResult = copyFeedback?.result;
  const Icon = copyResult === 'copied' ? Check : copyResult === 'failed' ? CircleAlert : Copy;
  return (
    <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-cyber-text-secondary">
      <button
        type="button"
        aria-label={t('mother.replyCopy')}
        className="inline-flex size-7 shrink-0 items-center justify-center rounded hover:bg-cyber-text/10 hover:text-cyber-text focus-visible:outline focus-visible:outline-1 focus-visible:outline-cyber-accent"
        onClick={async () =>
          setCopyFeedback({ result: (await copyText(text)) ? 'copied' : 'failed' })
        }
      >
        <Icon size={14} aria-hidden="true" />
      </button>
      <span role="status" className="sr-only">
        {copyResult && t(copyResult === 'copied' ? 'mother.replyCopied' : 'mother.replyCopyFailed')}
      </span>
      {metrics.length > 0 && <span className="tabular-nums">{metrics.join(' · ')}</span>}
    </div>
  );
}

// ── Main ChatBubble ───────────────────────────────────────────────────────────
export const ChatBubble = memo(function ChatBubble({
  role,
  content,
  chips = [],
  isStreaming = false,
  subContent,
  replySummary,
  workingStartedAt,
}: ChatBubbleProps) {
  if (role === 'skeleton') {
    return (
      <div className="mb-6 space-y-2">
        {[80, 60, 40].map((w, i) => (
          <div
            key={i}
            className="h-3 rounded-full animate-pulse"
            style={{ width: `${w}%`, background: 'rgba(255,255,255,0.08)' }}
          />
        ))}
      </div>
    );
  }

  if (role === 'system') {
    return (
      <div className="flex justify-center my-4">
        <span className="text-cyber-text-muted/50 text-xs font-mono">{content}</span>
      </div>
    );
  }

  if (role === 'error') {
    return (
      <div className="flex flex-col items-center gap-0.5 my-4">
        <span className="text-red-400 text-xs font-mono text-center">{content}</span>
        {subContent && (
          <span className="text-red-400/60 text-[11px] font-mono text-center">{subContent}</span>
        )}
      </div>
    );
  }

  if (role === 'working') {
    return <WorkingStatus startedAt={workingStartedAt} />;
  }

  // ── Assistant: full-width markdown, no bubble ─────────────────────────────
  if (role === 'assistant') {
    const cleaned = cleanAgentText(content);

    if (!cleaned) {
      return isStreaming ? <WorkingStatus startedAt={workingStartedAt} /> : null;
    }

    return (
      <div className="mb-6 text-sm leading-relaxed text-cyber-text font-sans">
        <div className="markdown-body">
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>
            {truncate(cleaned)}
          </ReactMarkdown>
          {isStreaming && (
            <span
              className="inline-block w-1.5 h-4 ml-0.5 align-text-bottom bg-cyber-accent"
              style={IS_LINUX ? undefined : { animation: 'caretBlink 1s steps(2) infinite' }}
            />
          )}
        </div>
        {!isStreaming && replySummary !== undefined && (
          <ReplyFooter text={cleaned} summary={replySummary} />
        )}
      </div>
    );
  }

  // ── User: subtle right-aligned card, plain text ──────────────────────────
  return (
    <div className="flex flex-col items-end mb-6">
      <div
        className="max-w-[78%] rounded-lg px-3.5 py-2 text-sm leading-relaxed font-sans whitespace-pre-line text-cyber-text border border-cyber-border/25 bg-cyber-text/5"
        style={{ overflowWrap: 'anywhere', wordBreak: 'break-word' }}
      >
        {truncate(content)}
      </div>
      {chips.length > 0 && <ReadonlyChips chips={chips} />}
    </div>
  );
});
