// Expand a tool row to inspect its full arguments and output.
import { memo, useLayoutEffect, useRef, useState } from 'react';
import {
  Loader2,
  Check,
  X as XIcon,
  Terminal,
  Globe,
  FileText,
  FileEdit,
  Upload,
  Download,
  KeyRound,
  Wrench,
  type LucideIcon,
} from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';

export interface ToolCallCardProps {
  name: string;
  args: string;
  status: 'running' | 'done' | 'failed';
  liveOutput?: string;
  output?: string;
}

// Map tool name → icon + a friendly label preview key
const TOOL_META: Record<string, { icon: LucideIcon; label: string; previewKey?: string }> = {
  shell_exec: { icon: Terminal, label: '', previewKey: 'command' },
  ssh_shell: { icon: Terminal, label: '', previewKey: 'command' },
  file_read: { icon: FileText, label: 'read', previewKey: 'path' },
  tool_output_read: { icon: FileText, label: 'read output' },
  conversation_history_read: { icon: FileText, label: 'read history' },
  file_write: { icon: FileEdit, label: 'write', previewKey: 'path' },
  web_fetch: { icon: Globe, label: 'fetch', previewKey: 'url' },
  upload_file: { icon: Upload, label: 'upload', previewKey: 'remote_path' },
  download_file: { icon: Download, label: 'download', previewKey: 'remote_path' },
  get_sudo_password: { icon: KeyRound, label: 'sudo', previewKey: 'server_id' },
  deploy_plugin_source: { icon: Wrench, label: 'deploy plugin', previewKey: 'plugin_id' },
};

function previewFor(toolName: string, args: string): string {
  const meta = TOOL_META[toolName];
  if (!meta?.previewKey) return '';
  try {
    const obj = JSON.parse(args);
    const val = obj?.[meta.previewKey];
    if (typeof val === 'string') return val;
  } catch {
    /* args still streaming — no parseable JSON yet */
  }
  return '';
}

// Close an unfinished string for display only. JSON.parse verifies the prefix
// belongs to the outer object and decodes only complete JSON escapes.
function streamedCommand(args: string): string {
  for (const field of args.matchAll(
    /"command"\s*:\s*("(?:[^"\\]|\\(?:["\\/bfnrt]|u[\da-fA-F]{4}))*)(")?/g
  )) {
    try {
      const value = JSON.parse(
        args.slice(0, field.index + field[0].length) + (field[2] ? '}' : '"}')
      );
      if (typeof value.command === 'string') return value.command.replace(/[\uD800-\uDBFF]$/, '');
    } catch {
      // A nested field or malformed prefix is not a previewable command.
    }
  }
  return '';
}

const OUTPUT_PREVIEW_LIMIT = 4000;

function CommandPreview({ text, following }: { text: string; following: boolean }) {
  const ref = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (element) element.scrollLeft = following ? element.scrollWidth : 0;
  }, [text, following]);

  return (
    <span
      ref={ref}
      dir="ltr"
      className={`min-w-0 flex-1 overflow-hidden whitespace-nowrap font-mono text-xs ${following ? '' : 'text-ellipsis'}`}
    >
      {text}
    </span>
  );
}

export const ToolCallCard = memo(function ToolCallCard({
  name,
  args,
  status,
  liveOutput,
  output,
}: ToolCallCardProps) {
  const [open, setOpen] = useState(false);
  const { t } = useI18n();

  const meta = TOOL_META[name] || { icon: Wrench, label: name };
  const Icon = meta.icon;
  const preview = previewFor(name, args);
  const isCommand = name === 'shell_exec' || name === 'ssh_shell';
  const runningCommand = isCommand && status === 'running';
  const command = isCommand ? (preview || streamedCommand(args)).trim() : '';
  if (runningCommand && !command) return null;

  const statusIcon =
    status === 'running' ? (
      <Loader2 size={11} className="motion-safe:animate-spin text-cyber-accent" />
    ) : status === 'failed' ? (
      <XIcon size={11} className="text-red-400" />
    ) : (
      <Check size={11} className="text-cyber-text-muted" />
    );

  const truncatedOutput =
    output && output.length > OUTPUT_PREVIEW_LIMIT
      ? output.slice(0, OUTPUT_PREVIEW_LIMIT) + '\n…\n[truncated]'
      : output;

  return (
    <div
      className={
        isCommand
          ? 'mb-1 text-[13px] text-cyber-text-secondary'
          : 'mb-3 rounded-md bg-cyber-surface/60 text-[12px] font-mono'
      }
    >
      <button
        type="button"
        aria-expanded={open}
        aria-busy={runningCommand || undefined}
        onClick={() => setOpen((o) => !o)}
        className={
          isCommand
            ? 'flex min-h-7 w-full min-w-0 items-center gap-2 rounded px-1.5 text-left bg-cyber-text/5 focus-visible:outline focus-visible:outline-1 focus-visible:outline-cyber-accent'
            : 'w-full flex items-center gap-2 px-2.5 py-1.5 text-left hover:bg-cyber-elevated/70 transition-colors rounded-md'
        }
      >
        <Icon
          size={isCommand ? 13 : 12}
          aria-hidden="true"
          className="shrink-0 text-cyber-text-muted"
        />
        {isCommand ? (
          <>
            <span className="shrink-0">{t('mother.command')}</span>
            <span className="shrink-0 text-cyber-text-muted" aria-hidden="true">
              ·
            </span>
            <CommandPreview
              following={runningCommand}
              text={(runningCommand && liveOutput) || command || t('mother.commandUnavailable')}
            />
            {status === 'failed' && (
              <span className="shrink-0" aria-hidden="true">
                {statusIcon}
              </span>
            )}
          </>
        ) : (
          <>
            <span className="flex-shrink-0 text-cyber-text font-bold">{meta.label}</span>
            {preview && <span className="flex-1 truncate text-cyber-text-muted/70">{preview}</span>}
            <span className="flex-shrink-0">{statusIcon}</span>
          </>
        )}
      </button>

      {open && (
        <div className="px-2.5 pb-2 pt-1 space-y-2 border-t border-cyber-border/30">
          <div>
            <div className="text-[10px] text-cyber-text-muted/60 mb-0.5">arguments</div>
            <pre className="bg-cyber-bg/60 px-2 py-1 rounded text-cyber-text whitespace-pre-wrap break-all">
              {args || '<streaming…>'}
            </pre>
          </div>
          {truncatedOutput !== undefined && (
            <div>
              <div className="text-[10px] text-cyber-text-muted/60 mb-0.5">output</div>
              <pre
                className={`px-2 py-1 rounded whitespace-pre-wrap break-all max-h-72 overflow-y-auto ${status === 'failed' ? 'bg-red-950/30 text-red-300' : 'bg-cyber-bg/60 text-cyber-text'}`}
              >
                {truncatedOutput}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
});
