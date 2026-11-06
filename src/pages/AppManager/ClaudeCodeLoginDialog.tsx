import { useEffect, useState } from 'react';
import { X } from 'lucide-react';
import { readText } from '@tauri-apps/plugin-clipboard-manager';
import { useI18n } from '../../hooks/useI18n';

export function ClaudeCodeLoginDialog({
  submitting,
  error,
  onClose,
  onSubmit,
}: {
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (code: string) => Promise<void>;
}) {
  const { t } = useI18n();
  const [code, setCode] = useState('');
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-[9998] flex items-center justify-center">
      <div className="absolute inset-0" onClick={onClose} />
      <form
        role="dialog"
        aria-modal="true"
        aria-labelledby="claude-code-login-title"
        className="relative w-[450px] max-w-[90vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden"
        onSubmit={(event) => {
          event.preventDefault();
          if (code.trim() && !submitting) void onSubmit(code);
        }}
      >
        <div className="h-px w-full bg-cyber-border" />
        <div className="px-6 pt-5 pb-4 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-cyber-text font-mono text-sm opacity-60">&gt;_</span>
            <span id="claude-code-login-title" className="text-base font-bold text-cyber-text">
              Claude Code
            </span>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('btn.cancel')}
            className="text-cyber-text-secondary close-button transition-colors"
          >
            <X size={18} />
          </button>
        </div>
        <div className="px-5 pb-5">
          <label
            htmlFor="claude-code-auth-code"
            className="block text-xs text-cyber-text-secondary mb-1"
          >
            {t('agent.authorizationCode')}
          </label>
          <div className="relative">
            <input
              id="claude-code-auth-code"
              type="text"
              value={code}
              onChange={(event) => setCode(event.target.value)}
              autoFocus
              autoComplete="off"
              spellCheck={false}
              disabled={submitting}
              className="w-full bg-cyber-input border border-cyber-border px-2 py-1.5 pr-16 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button"
            />
            <button
              type="button"
              disabled={submitting}
              onClick={async () => {
                try {
                  const text = (await readText()).trim();
                  if (text) setCode(text);
                } catch {
                  /* Match the existing input dialog. */
                }
              }}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-cyber-text-secondary"
            >
              {t('model.paste')}
            </button>
          </div>
          {error && (
            <p role="alert" className="pt-3 text-xs text-red-400 break-words">
              {error}
            </p>
          )}
        </div>
        <div className="flex border-t border-cyber-border">
          <button
            type="button"
            onClick={onClose}
            className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated transition-all border-r border-cyber-border"
          >
            {t('btn.cancel')}
          </button>
          <button
            type="submit"
            disabled={submitting || !code.trim()}
            className={`flex-1 px-4 py-3 text-[14px] font-semibold transition-all ${
              submitting || !code.trim()
                ? 'text-cyber-text-muted'
                : 'text-cyber-text hover:bg-cyber-text/10'
            }`}
          >
            {t('common.confirm')}
          </button>
        </div>
      </form>
    </div>
  );
}
