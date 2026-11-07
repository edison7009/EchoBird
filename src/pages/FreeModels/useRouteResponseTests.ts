import { useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  cancelSmartRouterProbe,
  testSmartRouterCandidate,
  type SmartRouterProbeResult,
} from '../../api/smartRouter';
import type { RouteModelNode } from './provider';
import { finishConnectionTestBatch } from '../../api/models';

function binding(model: RouteModelNode) {
  return JSON.stringify([
    model.modelId,
    model.baseUrl,
    model.anthropicUrl,
    model.apiKey,
    model.accountSource,
  ]);
}

export function useRouteResponseTests(models: RouteModelNode[], active: boolean, routerId = '') {
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());
  const [savedResults, setSavedResults] = useState<
    Record<string, { binding: string; result: SmartRouterProbeResult }>
  >({});
  const [resultRouterId, setResultRouterId] = useState(routerId);
  if (resultRouterId !== routerId) {
    setResultRouterId(routerId);
    setSavedResults({});
  }
  const generation = useRef(0);
  const running = useRef(false);
  const requests = useRef(new Map<string, string>());
  const batchId = useRef<string | null>(null);
  const roundBindings = useRef(new Map<string, string>());
  const bindings = useMemo(
    () => new Map(models.map((model) => [model.id, JSON.stringify([routerId, binding(model)])])),
    [models, routerId]
  );

  const cancel = useCallback(() => {
    generation.current++;
    running.current = false;
    roundBindings.current.clear();
    for (const requestId of requests.current.values()) {
      void cancelSmartRouterProbe(requestId).catch(() => undefined);
    }
    requests.current.clear();
    if (batchId.current) {
      void finishConnectionTestBatch(batchId.current).catch(() => undefined);
      batchId.current = null;
    }
    setPendingIds(new Set());
  }, []);

  useLayoutEffect(() => cancel, [routerId, cancel]);
  useLayoutEffect(() => {
    if ([...roundBindings.current].some(([id, value]) => bindings.get(id) !== value)) cancel();
  }, [bindings, cancel]);

  const test = async () => {
    if (!active || running.current || models.length === 0) return;
    running.current = true;
    const attempt = ++generation.current;
    const batch = crypto.randomUUID();
    batchId.current = batch;
    roundBindings.current = new Map(bindings);
    const queue = models.map((model) => ({ model, binding: bindings.get(model.id)! }));
    let next = 0;
    setPendingIds(new Set(models.map((model) => model.id)));
    const worker = async () => {
      while (generation.current === attempt && next < queue.length) {
        const entry = queue[next++];
        const { model } = entry;
        const requestId = crypto.randomUUID();
        requests.current.set(model.id, requestId);
        let result: SmartRouterProbeResult;
        try {
          result = await testSmartRouterCandidate(
            model.internalId,
            model.modelId,
            requestId,
            batch,
            routerId || undefined
          );
        } catch {
          result = { latencyMs: null, error: 'request_failed' };
        }
        if (generation.current !== attempt) return;
        requests.current.delete(model.id);
        setSavedResults((previous) => ({
          ...previous,
          [model.id]: { binding: entry.binding, result },
        }));
        setPendingIds((previous) => {
          const remaining = new Set(previous);
          remaining.delete(model.id);
          return remaining;
        });
      }
    };
    try {
      await Promise.all([worker(), worker()]);
    } finally {
      if (batchId.current === batch) {
        batchId.current = null;
        void finishConnectionTestBatch(batch).catch(() => undefined);
      }
      if (generation.current === attempt) {
        running.current = false;
        roundBindings.current.clear();
      }
    }
  };

  const results: Record<string, SmartRouterProbeResult> = {};
  for (const [id, value] of Object.entries(savedResults)) {
    if (bindings.get(id) === value.binding) results[id] = value.result;
  }
  return { pendingIds, results, isTesting: pendingIds.size > 0, test };
}
