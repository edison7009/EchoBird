import { useCallback, useEffect, useMemo, useSyncExternalStore } from 'react';
import * as api from '../api/tauri';
import type { ModelConfig } from '../api/types';
import { modelRepository } from '../stores/modelRepository';

let owners = 0;
const refresh = () => {
  if (api.getModels) void modelRepository.load(api.getModels, true).catch(() => {});
};
const invalidate = (event: Event) => {
  if (event instanceof CustomEvent && event.detail?.repositoryUpdated) return;
  modelRepository.invalidate();
};
const all = () => true;

export function useModelRepository(filter: (model: ModelConfig) => boolean = all, active = true) {
  const snapshot = useSyncExternalStore(modelRepository.subscribe, modelRepository.getSnapshot);
  useEffect(() => {
    if (!active) return;
    owners++;
    if (owners === 1 && typeof window !== 'undefined') {
      window.addEventListener('focus', refresh);
      // Compatibility for native/local-server notifications; mutations publish directly.
      window.addEventListener('models-changed', invalidate);
    }
    if (api.getModels)
      void modelRepository
        .load(api.getModels, modelRepository.getSnapshot().loaded)
        .catch(() => {});
    return () => {
      owners--;
      if (!owners) {
        if (typeof window !== 'undefined') {
          window.removeEventListener('focus', refresh);
          window.removeEventListener('models-changed', invalidate);
        }
      }
    };
  }, [active]);
  const models = useMemo(() => snapshot.models.filter(filter), [snapshot.models, filter]);
  const setModels = useCallback(
    (update: ModelConfig[] | ((models: ModelConfig[]) => ModelConfig[])) =>
      modelRepository.setView(filter, update),
    [filter]
  );
  return {
    ...snapshot,
    models,
    setModels,
    reload: () => modelRepository.load(api.getModels, true),
  };
}
