import { useEffect, useState, type CSSProperties } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useI18n } from '../../hooks/useI18n';
import { THEME_CHANGED_EVENT, useThemeStore, type ThemeSelection } from '../../stores/themeStore';
import {
  getTrayUsage,
  readyTrayUsage,
  TRAY_USAGE_STATE_EVENT,
  type TrayUsagePreview,
  type TrayUsageSnapshot,
} from '../../api/usageIsland';
import {
  UsageIslandListRow,
  UsageIslandTexture,
  useUsageIslandAppearance,
} from './UsageIslandView';
import './UsageIsland.css';

export function TrayUsageList({ preview }: { preview: TrayUsagePreview }) {
  const { t } = useI18n();
  const { className } = useUsageIslandAppearance(preview.theme);
  return (
    <div
      className={`${className} tray-usage-preview`}
      data-layout="list"
      style={{ '--usage-island-surface-opacity': `${preview.opacity ?? 100}%` } as CSSProperties}
      role={preview.items.length ? 'list' : undefined}
    >
      <UsageIslandTexture theme={preview.theme} background={preview.background} />
      {preview.items.length === 0 && (
        <div className="tray-usage-empty">{t('usageIsland.trayEmpty')}</div>
      )}
      {preview.items.map((item) => (
        <div
          className="usage-island-tool"
          data-smart-active={item.smartActive || undefined}
          role="listitem"
          key={item.key}
        >
          <UsageIslandListRow item={item} />
        </div>
      ))}
    </div>
  );
}

export function TrayUsageWindow() {
  const [snapshot, setSnapshot] = useState<TrayUsageSnapshot | null>(null);
  useEffect(() => {
    let disposed = false;
    const accept = (next: TrayUsageSnapshot | null) => {
      if (!disposed && next) {
        setSnapshot((current) => (!current || next.revision > current.revision ? next : current));
      }
    };
    const subscription = listen<TrayUsageSnapshot>(TRAY_USAGE_STATE_EVENT, ({ payload }) => {
      accept(payload);
    })
      .then((unlisten) => {
        if (!disposed)
          void getTrayUsage()
            .then(accept)
            .catch(() => {});
        return unlisten;
      })
      .catch(() => undefined);
    const themeSubscription = listen<ThemeSelection>(THEME_CHANGED_EVENT, ({ payload }) => {
      if (!disposed) useThemeStore.getState().sync(payload);
    }).catch(() => undefined);
    return () => {
      disposed = true;
      void subscription.then((unlisten) => unlisten?.());
      void themeSubscription.then((unlisten) => unlisten?.());
    };
  }, []);
  useEffect(() => {
    if (snapshot?.preview) void readyTrayUsage(snapshot.revision).catch(() => {});
  }, [snapshot]);

  return snapshot?.preview && <TrayUsageList preview={snapshot.preview} />;
}
