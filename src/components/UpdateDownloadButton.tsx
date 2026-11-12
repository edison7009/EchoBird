import { Download, RotateCw } from 'lucide-react';
import type { SelfUpdateProgress } from '../api/tauri';
import { useI18n } from '../hooks/useI18n';
import { RoutingHint } from './RoutingToggle';

export function UpdateDownloadButton({
  version,
  progress,
  collapsed,
  onClick,
}: {
  version: string;
  collapsed: boolean;
  progress: SelfUpdateProgress | null;
  onClick: () => void;
}) {
  const { t } = useI18n();
  const percent = Math.max(0, Math.min(100, progress?.percent ?? 0));
  const restarting = progress?.status === 'launching';
  const label = !progress
    ? `${t('updates.download')} v${version}`
    : progress.status === 'speed_test'
      ? t('updates.connecting')
      : restarting
        ? t('updates.restarting')
        : `${t('updates.downloading')} ${percent}%`;
  return (
    <RoutingHint hint={label} placement={collapsed ? 'right' : 'top'} navigation>
      <button
        type="button"
        aria-label={label}
        aria-busy={!!progress}
        disabled={!!progress}
        onClick={onClick}
        className="relative mx-auto flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-red-400 transition-colors hover:bg-cyber-elevated/50"
      >
        {restarting ? (
          <RotateCw size={20} aria-hidden="true" />
        ) : progress ? (
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
    </RoutingHint>
  );
}
