import { useEffect, useRef, useState } from 'react';
import * as api from '../api/tauri';
import { useToast } from '../components/Toast';
import { useI18n } from './useI18n';
import { fetchLatestManifest, isNewerVersion } from '../services/updateManifest';
import { useNavigationStore } from '../stores/navigationStore';

// Owned by the sidebar, so folding the rail never resets a running download.
export function useSelfUpdateDownload(version: string | null) {
  const { showToast } = useToast();
  const { t } = useI18n();
  const [progress, setProgress] = useState<api.SelfUpdateProgress | null>(null);
  const [readyVersion, setReadyVersion] = useState<string | null>(null);
  const busy = progress !== null;
  const pending = useRef(false);
  const generation = useRef(0);
  const mounted = useRef(true);
  const listener = useRef<(() => void) | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      listener.current?.();
      listener.current = null;
    };
  }, []);
  useEffect(() => {
    if (!version || busy || readyVersion === version) return;
    let active = true;
    const request = generation.current;
    void api.isUpdateDownloaded(version).then(
      (ready) => {
        if (ready && active && request === generation.current) setReadyVersion(version);
      },
      () => {}
    );
    return () => {
      active = false;
    };
  }, [busy, readyVersion, version]);

  const activate = async (version: string) => {
    if (pending.current) return;
    pending.current = true;
    const request = ++generation.current;
    const installing = readyVersion === version;
    setProgress(
      installing ? { status: 'launching', percent: 100 } : { status: 'speed_test', percent: 0 }
    );
    let unlisten: (() => void) | undefined;
    try {
      const release = await api.onSelfUpdateProgress((event) => {
        if (mounted.current && pending.current && request === generation.current)
          setProgress(event);
      });
      let closed = false;
      unlisten = () => {
        if (!closed) {
          closed = true;
          release();
        }
      };
      if (!mounted.current) return;
      listener.current = unlisten;
      if (installing) {
        const opened = await api.installDownloadedUpdate(version);
        if (!opened && mounted.current) {
          setReadyVersion(null);
          showToast('error', t('updates.failed'));
        }
        return;
      }
      const latest = await fetchLatestManifest(undefined, 4000);
      if (!mounted.current) return;
      const target = latest && isNewerVersion(latest.version, version) ? latest.version : version;
      if (target !== version) useNavigationStore.getState().setUpdateAvailable(target);
      const ready = await api.downloadUpdate(target);
      if (mounted.current) setReadyVersion(ready ? target : null);
    } catch {
      if (mounted.current) showToast('error', t('updates.failed'));
    } finally {
      if (listener.current === unlisten) listener.current = null;
      unlisten?.();
      pending.current = false;
      if (mounted.current) setProgress(null);
    }
  };
  return { progress, ready: version !== null && readyVersion === version, activate };
}
