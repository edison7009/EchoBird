import { Download } from 'lucide-react';
import type { SelfUpdateProgress } from '../api/tauri';
import { useI18n } from '../hooks/useI18n';

export function UpdateDownloadButton({
  version,
  progress,
  onClick,
}: {
  version: string;
  progress: SelfUpdateProgress | null;
  onClick: () => void;
}) {
  const { t } = useI18n();
  const percent = Math.max(0, Math.min(100, progress?.percent ?? 0));
  return (
    <button
      type="button"
      aria-label={`${t(progress ? 'updates.downloading' : 'updates.download')} ${version}${progress ? ` · ${percent}%` : ''}`}
      aria-busy={!!progress}
      disabled={!!progress}
      onClick={onClick}
      className="relative mx-auto flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-red-400 transition-colors hover:bg-cyber-elevated/50"
    >
      {progress ? (
        <svg
          aria-hidden="true"
          className={
            progress.status === 'speed_test'
              ? 'h-[15px] w-[15px] motion-safe:animate-spin motion-reduce:animate-none'
              : 'h-[15px] w-[15px]'
          }
          viewBox="0 0 24 24"
        >
          <circle
            cx="12"
            cy="12"
            r="9"
            fill="none"
            stroke="currentColor"
            strokeOpacity="0.22"
            strokeWidth="2.6"
          />
          <circle
            cx="12"
            cy="12"
            r="9"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.6"
            strokeLinecap="round"
            transform="rotate(-90 12 12)"
            strokeDasharray={
              progress.status === 'speed_test'
                ? `${2 * Math.PI * 9 * 0.25} ${2 * Math.PI * 9}`
                : 2 * Math.PI * 9
            }
            strokeDashoffset={
              progress.status === 'speed_test' ? 0 : 2 * Math.PI * 9 * (1 - percent / 100)
            }
          />
        </svg>
      ) : (
        <Download size={20} aria-hidden="true" />
      )}
    </button>
  );
}
