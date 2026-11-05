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
      <Download
        size={20}
        aria-hidden="true"
        className={
          progress?.status === 'speed_test' ? 'animate-pulse motion-reduce:animate-none' : undefined
        }
      />
      {progress && (
        <svg
          aria-hidden="true"
          className="absolute inset-0 h-full w-full -rotate-90"
          viewBox="0 0 40 40"
        >
          <circle
            cx="20"
            cy="20"
            r="17"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            pathLength="100"
            strokeDasharray={`${percent} 100`}
          />
        </svg>
      )}
    </button>
  );
}
