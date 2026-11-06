import { useModelRepository } from '../../hooks/useModelRepository';
import { isSmartRouterId } from '../../utils/smartRouter';
// Model Nexus Page — Model cards, debug console, add/edit modal
// Extracted from App.tsx with Provider pattern for shared state
import { useState, useEffect, useRef, useCallback } from 'react';
import { open as shellOpen } from '@tauri-apps/plugin-shell';
import { readText as readClipboardText } from '@tauri-apps/plugin-clipboard-manager';
import { X } from 'lucide-react';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { ModelConfig } from '../../api/types';
import { ModelNexusContext } from './context';
import type { NewModelForm, UsageRefreshOptions } from './context';
import type { ModelUsageData } from '../../api/tauri';

// ===== Provider =====

/** Whether a model's endpoint is Volcengine (cn) - needs AK/SK for usage.
 *  Mirrors the backend can_handle + URL-selection (baseUrl if non-empty, else anthropicUrl). */
export const isVolcengineUrl = (baseUrl: string, anthropicUrl?: string | null) => {
  try {
    const url = new URL(baseUrl || anthropicUrl || '');
    return (
      url.protocol === 'https:' &&
      url.hostname === 'ark.cn-beijing.volces.com' &&
      !url.username &&
      !url.password &&
      !url.port &&
      /^\/api\/coding(?:\/v[13])?$/.test(url.pathname.replace(/\/+$/, ''))
    );
  } catch {
    return false;
  }
};

const visibleModelNexusModel = (model: ModelConfig) =>
  model.internalId !== 'local-server' &&
  !isSmartRouterId(model.internalId) &&
  (!model.scope || model.scope === 'modelCenter');
const visibleModelNexusModels = (models: ModelConfig[]) => models.filter(visibleModelNexusModel);

export function ModelNexusProvider({ children }: { children: React.ReactNode }) {
  const { t } = useI18n();
  // Models state
  const repository = useModelRepository(visibleModelNexusModel);
  const { models: userModels, setModels: setUserModels } = repository;
  const isLoadingModels = repository.loading;
  const [selectedModel, setSelectedModel] = useState<string | null>('gpt4o');
  const [pingingModelIds, setPingingModelIds] = useState<Set<string>>(new Set());

  // View mode state
  const [viewMode, setViewMode] = useState<'config' | 'usage'>('config');
  const [modelUsageData, setModelUsageData] = useState<Record<string, ModelUsageData>>({});
  const usageAccessRevision = useRef<Record<string, number>>({});
  const editingUsageModel = useRef<ModelConfig | null>(null);
  const usageBatchPending = useRef(false);
  const usageRequests = useRef(new Map<string, number>());
  const [isRefreshingUsage, setIsRefreshingUsage] = useState(false);
  const [refreshingUsageIds, setRefreshingUsageIds] = useState<Set<string>>(new Set());

  const invalidateModelUsage = useCallback((modelId: string) => {
    usageAccessRevision.current[modelId] = (usageAccessRevision.current[modelId] ?? 0) + 1;
    setModelUsageData((previous) => {
      const next = { ...previous };
      delete next[modelId];
      return next;
    });
    setRefreshingUsageIds((previous) => {
      const next = new Set(previous);
      next.delete(modelId);
      return next;
    });
  }, []);

  const updateUserModel = (updated: ModelConfig, form: NewModelForm) => {
    const previous = editingUsageModel.current;
    if (
      !previous ||
      previous.internalId !== updated.internalId ||
      previous.apiKey !== form.apiKey ||
      previous.baseUrl !== form.baseUrl ||
      (previous.anthropicUrl || '') !== form.anthropicUrl
    ) {
      invalidateModelUsage(updated.internalId);
    }
    setUserModels((models) =>
      models.map((model) => (model.internalId === updated.internalId ? updated : model))
    );
  };
  // Volcengine AK/SK (per-model: one account per model)
  const { showToast } = useToast();
  const [volcAkSkMissingIds, setVolcAkSkMissingIds] = useState<Set<string>>(new Set());
  const [volcAkSkModelId, setVolcAkSkModelId] = useState<string | null>(null);
  const [zhipuTeamModal, setZhipuTeamModal] = useState<{
    modelId: string;
    access: api.ZhipuTeamAccess | null;
  } | null>(null);

  // Modal state
  const [showAddModelModal, setShowAddModelModal] = useState(false);
  const [modelModalDestination, setModelModalDestination] = useState<
    'modelNexus' | 'freeRouter' | 'voiceInput'
  >('modelNexus');
  const [modelModalAnimatingOut, setModelModalAnimatingOut] = useState(false);
  const [editingModelId, setEditingModelId] = useState<string | null>(null);
  const [showApiKey, setShowApiKey] = useState(false);
  const [keyDestroyed, setKeyDestroyed] = useState(false);
  const [newModelForm, setNewModelForm] = useState<NewModelForm>({
    name: '',
    baseUrl: '',
    anthropicUrl: '',
    apiKey: '',
    modelId: '',
  });

  const closeModelModal = useCallback(() => {
    setModelModalAnimatingOut(true);
    setTimeout(() => {
      setModelModalAnimatingOut(false);
      setShowAddModelModal(false);
      setEditingModelId(null);
      editingUsageModel.current = null;
      setModelModalDestination('modelNexus');
    }, 200);
  }, []);

  // Pre-fill values for the AK/SK modal (fetched when opening for a model).
  const [volcAkSkInitial, setVolcAkSkInitial] = useState<{
    access_key: string;
    secret_key: string;
  } | null>(null);

  const openAkskModal = async (internalId: string) => {
    try {
      setVolcAkSkInitial(await api.getVolcAksk(internalId));
    } catch {
      setVolcAkSkInitial(null);
    }
    setVolcAkSkModelId(internalId);
  };

  const openZhipuTeamModal = async (modelId: string) => {
    try {
      const access = await api.getZhipuTeamAccess(modelId);
      setZhipuTeamModal({ modelId, access });
    } catch {
      showToast('error', t('model.quota.accessFailed'));
    }
  };

  const handleCardEdit = useCallback(
    async (model: (typeof userModels)[0]) => {
      // Reload fresh model data from disk to get latest apiKey state
      let freshModel = model;
      try {
        const freshModels = await repository.reload();
        const found = freshModels.find((m) => m.internalId === model.internalId);
        if (found) {
          freshModel = found;
          // Also update the models list with fresh data
          setUserModels(visibleModelNexusModels(freshModels));
        }
      } catch {
        /* fallback to stale model */
      }

      editingUsageModel.current = freshModel;
      setModelModalDestination('modelNexus');
      setEditingModelId(freshModel.internalId);
      if (freshModel.apiKey?.startsWith('enc:v1:') && api.isKeyDestroyed) {
        api.isKeyDestroyed(freshModel.internalId).then((destroyed) => setKeyDestroyed(destroyed));
      } else {
        setKeyDestroyed(false);
      }
      setNewModelForm({
        name: freshModel.name,
        baseUrl: freshModel.baseUrl,
        anthropicUrl: freshModel.anthropicUrl || '',
        apiKey: freshModel.apiKey,
        modelId: freshModel.modelId || '',
        accountSource: freshModel.accountSource,
      });
      setShowAddModelModal(true);
    },
    [
      repository,
      setEditingModelId,
      setKeyDestroyed,
      setNewModelForm,
      setShowAddModelModal,
      setUserModels,
    ]
  );

  const handleCardDelete = useCallback(
    async (modelId: string) => {
      try {
        await api.deleteModel(modelId);
        invalidateModelUsage(modelId);
        setUserModels((prev) => prev.filter((m) => m.internalId !== modelId));
      } catch (error) {
        showToast('error', typeof error === 'string' ? error : String(error));
      }
    },
    [setUserModels, invalidateModelUsage, showToast]
  );

  // Test state
  const [testInput, setTestInput] = useState('');
  const [testOutput, setTestOutput] = useState<string[]>([]);
  const [isTesting, setIsTesting] = useState(false);
  const [modelLatencies, setModelLatencies] = useState<Record<string, number>>({});
  const [modelTerminals, setModelTerminals] = useState<
    Record<string, { input: string; output: string[] }>
  >({});
  const [testProtocol, setTestProtocol] = useState<'openai' | 'anthropic'>('openai');
  const testInputRef = useRef<HTMLInputElement>(null!);
  const [inputFocused, setInputFocused] = useState(false);
  const [cursorPos, setCursorPos] = useState(0);

  // Derived
  const selectedModelData = userModels.find((m) => m.internalId === selectedModel);

  // Auto-fill Model ID and API Key for local models
  useEffect(() => {
    const isLocal = (url: string) => url.includes('localhost') || url.includes('127.0.0.1');
    const hasLocalUrl = isLocal(newModelForm.baseUrl) || isLocal(newModelForm.anthropicUrl);

    if (hasLocalUrl) {
      setNewModelForm((prev) => {
        const updates: { modelId?: string; apiKey?: string } = {};
        if (!prev.modelId) updates.modelId = 'local-model';
        if (!prev.apiKey) updates.apiKey = 'not-needed';
        return Object.keys(updates).length > 0 ? { ...prev, ...updates } : prev;
      });
    }
  }, [newModelForm.baseUrl, newModelForm.anthropicUrl]);

  // Listen for model selection change - auto restore terminal history and focus
  useEffect(() => {
    if (selectedModel && modelTerminals[selectedModel]) {
      const saved = modelTerminals[selectedModel];
      setTestInput(saved?.input || '');
      setTestOutput(saved?.output || []);
    } else {
      setTestInput('');
      setTestOutput([]);
    }
    testInputRef.current?.focus();
    // Restore terminal history only when the selected model changes; reading the
    // latest modelTerminals here is intentional (no re-run on map updates).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedModel]);

  // Listen for protocol change - focus input
  useEffect(() => {
    testInputRef.current?.focus();
  }, [testProtocol]);

  // ping --all
  const pingAllModels = async () => {
    if (isTesting) return;
    setIsTesting(true);
    const allModels = userModels;
    const batchId = crypto.randomUUID();
    setPingingModelIds(new Set(allModels.map((m) => m.internalId)));
    try {
      for (const model of allModels) {
        try {
          const result = await api.pingModel(model.internalId, batchId);
          setPingingModelIds((prev) => {
            const next = new Set(prev);
            next.delete(model.internalId);
            return next;
          });
          // Keep timeouts (-2) distinct from other tested failures (-1).
          setModelLatencies((prev) => ({
            ...prev,
            [model.internalId]:
              result?.error === 'timeout' || (result?.success && result.latency > 1000)
                ? -2
                : result?.success
                  ? result.latency
                  : -1,
          }));
        } catch {
          setPingingModelIds((prev) => {
            const next = new Set(prev);
            next.delete(model.internalId);
            return next;
          });
          setModelLatencies((prev) => ({ ...prev, [model.internalId]: -1 }));
        }
      }
    } finally {
      void api.finishConnectionTestBatch(batchId).catch(() => undefined);
      setPingingModelIds(new Set());
      setIsTesting(false);
    }
  };

  // Refresh usage for all models
  const refreshAllUsage = useCallback(async () => {
    if (usageBatchPending.current) return;
    const models = userModels.filter(
      (model) =>
        usageRequests.current.get(model.internalId) !==
        (usageAccessRevision.current[model.internalId] ?? 0)
    );
    if (!models.length) return;
    usageBatchPending.current = true;
    setIsRefreshingUsage(true);
    const revisions = models.map((model) => usageAccessRevision.current[model.internalId] ?? 0);
    models.forEach((model, i) => usageRequests.current.set(model.internalId, revisions[i]));
    setRefreshingUsageIds(
      (previous) => new Set([...previous, ...models.map((model) => model.internalId)])
    );
    try {
      // Failed models keep their last data; obsolete configurations cannot publish results.
      const results = await Promise.allSettled(
        models.map(async (model) => api.queryModelUsage(model.internalId))
      );
      setModelUsageData((prev) => {
        const next = { ...prev };
        results.forEach((r, i) => {
          if (
            r.status === 'fulfilled' &&
            r.value.success &&
            r.value.data &&
            revisions[i] === (usageAccessRevision.current[models[i].internalId] ?? 0)
          ) {
            next[models[i].internalId] = r.value.data;
          }
        });
        return next;
      });
      const failed = models.filter(
        (model, i) =>
          revisions[i] === (usageAccessRevision.current[model.internalId] ?? 0) &&
          !(results[i].status === 'fulfilled' && results[i].value.success && results[i].value.data)
      );
      if (failed.length) {
        showToast(
          'error',
          `${t('model.quota.refreshFailed').replace('{n}', String(failed.length))} ${failed.map((model) => model.name).join(', ')}`
        );
      }
    } finally {
      models.forEach((model, i) => {
        if (usageRequests.current.get(model.internalId) === revisions[i]) {
          usageRequests.current.delete(model.internalId);
        }
      });
      setRefreshingUsageIds((previous) => {
        const next = new Set(previous);
        models.forEach((model, i) => {
          if (revisions[i] === (usageAccessRevision.current[model.internalId] ?? 0)) {
            next.delete(model.internalId);
          }
        });
        return next;
      });
      usageBatchPending.current = false;
      setIsRefreshingUsage(false);
    }
  }, [userModels, showToast, t]);

  // Refresh usage for a single model
  const refreshSingleUsage = async (modelId: string, options?: UsageRefreshOptions) => {
    const model = userModels.find((m) => m.internalId === modelId);
    if (!model) return;

    const revision = usageAccessRevision.current[modelId] ?? 0;
    if (usageRequests.current.get(modelId) === revision) return;
    usageRequests.current.set(modelId, revision);
    setRefreshingUsageIds((prev) => new Set(prev).add(model.internalId));
    try {
      const result = await api.queryModelUsage(model.internalId);
      if (revision !== (usageAccessRevision.current[modelId] ?? 0)) return;
      if (result.error === 'VOLC_AKSK_REQUIRED') {
        setVolcAkSkMissingIds((prev) => new Set(prev).add(model.internalId));
        return;
      }
      if (result.success && result.data) {
        const data = result.data;
        setModelUsageData((prev) => ({
          ...prev,
          [model.internalId]: data,
        }));
        setVolcAkSkMissingIds((prev) => {
          if (!prev.has(model.internalId)) return prev;
          const next = new Set(prev);
          next.delete(model.internalId);
          return next;
        });
      } else if (result.error) {
        options?.onError?.(result.error);
        if (!options?.silent) showToast('error', result.error);
      }
    } catch (error) {
      options?.onError?.(error);
      if (!options?.silent && revision === (usageAccessRevision.current[modelId] ?? 0)) {
        showToast('error', typeof error === 'string' ? error : t('error.requestFailed'));
      }
    } finally {
      if (usageRequests.current.get(modelId) === revision) usageRequests.current.delete(modelId);
      setRefreshingUsageIds((prev) => {
        if (revision !== (usageAccessRevision.current[modelId] ?? 0)) return prev;
        const next = new Set(prev);
        next.delete(model.internalId);
        return next;
      });
    }
  };

  // On mount / when models load, mark which Volcengine models lack AK/SK.
  useEffect(() => {
    const volcModels = userModels.filter((m) => isVolcengineUrl(m.baseUrl, m.anthropicUrl));
    Promise.all(
      volcModels.map(async (m) => {
        try {
          return { id: m.internalId, missing: !(await api.hasVolcAksk(m.internalId)) };
        } catch {
          return { id: m.internalId, missing: true };
        }
      })
    ).then((results) => {
      setVolcAkSkMissingIds(new Set(results.filter((r) => r.missing).map((r) => r.id)));
    });
  }, [userModels]);

  // Save AK/SK for a specific model, then refresh that model's usage.
  const saveVolcAksk = async (internalId: string, accessKey: string, secretKey: string) => {
    try {
      await api.saveVolcAksk(internalId, accessKey, secretKey);
      invalidateModelUsage(internalId);
      setVolcAkSkMissingIds((prev) => {
        const next = new Set(prev);
        next.delete(internalId);
        return next;
      });
      setVolcAkSkModelId(null);
      await refreshSingleUsage(internalId);
    } catch (e) {
      showToast('error', typeof e === 'string' ? e : String(e));
    }
  };

  // Model test function
  const handleTestModel = async () => {
    if (!testInput.trim() || !selectedModel || isTesting) return;

    const prompt = testInput.trim();
    setTestInput('');
    setIsTesting(true);
    testInputRef.current?.blur();

    // Smart protocol selection
    let effectiveProtocol = testProtocol;
    if (selectedModelData) {
      if (!selectedModelData.baseUrl && selectedModelData.anthropicUrl) {
        effectiveProtocol = 'anthropic';
      } else if (selectedModelData.baseUrl && !selectedModelData.anthropicUrl) {
        effectiveProtocol = 'openai';
      }
    }

    setTestOutput((prev) => [
      ...prev,
      `> ${prompt}`,
      `Sending request via ${effectiveProtocol === 'openai' ? 'OpenAI' : 'Anthropic'}...`,
    ]);

    try {
      if (!api.testModel) {
        setTestOutput((prev) => [...prev, 'Test API not available']);
        return;
      }

      const result = await api.testModel(selectedModel, prompt, effectiveProtocol);

      if (result.success) {
        setModelLatencies((prev) => ({ ...prev, [selectedModel]: result.latency }));
        setTestOutput((prev) => [
          ...prev,
          `Response in ${result.latency}ms`,
          result.response || 'No response',
        ]);
        // Reload model list to refresh test status
        await repository.reload();
      } else {
        // Sentinel -1 so the model card shows "Error" instead of the
        // pre-test "未测试" placeholder — see pingAllModels for the
        // same reasoning.
        setModelLatencies((prev) => ({ ...prev, [selectedModel]: -1 }));
        setTestOutput((prev) =>
          [
            ...prev,
            result.error || 'Unknown error',
            result.latency > 0 ? `(failed after ${result.latency}ms)` : '',
          ].filter(Boolean)
        );
      }
    } catch (error) {
      setModelLatencies((prev) => ({ ...prev, [selectedModel]: -1 }));
      setTestOutput((prev) => [...prev, String(error)]);
    } finally {
      setIsTesting(false);
    }
  };

  return (
    <ModelNexusContext.Provider
      value={{
        userModels,
        setUserModels,
        updateUserModel,
        isLoadingModels,
        selectedModel,
        setSelectedModel,
        selectedModelData,
        viewMode,
        setViewMode,
        modelUsageData,
        isRefreshingUsage,
        refreshingUsageIds,
        setModelUsageData,
        volcAkSkMissingIds,
        setVolcAkSkMissingIds,
        volcAkSkModelId,
        setVolcAkSkModelId,
        saveVolcAksk,
        testInput,
        setTestInput,
        testOutput,
        setTestOutput,
        isTesting,
        testProtocol,
        setTestProtocol,
        modelLatencies,
        pingingModelIds,
        modelTerminals,
        setModelTerminals,
        testInputRef,
        inputFocused,
        setInputFocused,
        cursorPos,
        setCursorPos,
        showAddModelModal,
        setShowAddModelModal,
        modelModalDestination,
        setModelModalDestination,
        modelModalAnimatingOut,
        editingModelId,
        setEditingModelId,
        newModelForm,
        setNewModelForm,
        showApiKey,
        setShowApiKey,
        keyDestroyed,
        setKeyDestroyed,
        closeModelModal,
        pingAllModels,
        refreshAllUsage,
        refreshSingleUsage,
        handleTestModel,
        handleCardEdit,
        handleCardDelete,
        openAkskModal,
        openZhipuTeamModal,
      }}
    >
      {children}
      {volcAkSkModelId && (
        <UsageAccessModal
          onClose={() => setVolcAkSkModelId(null)}
          onSave={(ak, sk) => saveVolcAksk(volcAkSkModelId, ak, sk)}
          initialAk={volcAkSkInitial?.access_key ?? ''}
          initialSk={volcAkSkInitial?.secret_key ?? ''}
        />
      )}
      {zhipuTeamModal && (
        <UsageAccessModal
          key={zhipuTeamModal.modelId}
          team
          onClose={() => setZhipuTeamModal(null)}
          initialAk={zhipuTeamModal.access?.organizationId ?? ''}
          initialSk={zhipuTeamModal.access?.projectId ?? ''}
          onSave={async (organizationId, projectId) => {
            try {
              await api.saveZhipuTeamAccess(
                zhipuTeamModal.modelId,
                organizationId && projectId ? { organizationId, projectId } : null
              );
              invalidateModelUsage(zhipuTeamModal.modelId);
              setZhipuTeamModal(null);
              await refreshSingleUsage(zhipuTeamModal.modelId);
            } catch {
              showToast('error', t('model.quota.accessFailed'));
            }
          }}
        />
      )}
    </ModelNexusContext.Provider>
  );
}

// ===== Main Content (model card grid) =====

// Two-field usage access configuration, mounted fresh for each model.
function UsageAccessModal({
  onClose,
  onSave,
  initialAk = '',
  initialSk = '',
  team = false,
}: {
  onClose: () => void;
  onSave: (accessKey: string, secretKey: string) => Promise<void>;
  initialAk?: string;
  initialSk?: string;
  team?: boolean;
}) {
  const { t } = useI18n();
  const [ak, setAk] = useState(initialAk);
  const [sk, setSk] = useState(initialSk);
  const [saving, setSaving] = useState(false);
  const valid = team ? Boolean(ak.trim()) === Boolean(sk.trim()) : Boolean(ak.trim() && sk.trim());
  const accessUrl = team
    ? 'https://open.bigmodel.cn'
    : 'https://console.volcengine.com/iam/keymanage';

  const handleSave = async () => {
    if (!valid || saving) return;
    setSaving(true);
    try {
      await onSave(ak.trim(), sk.trim());
    } finally {
      setSaving(false);
    }
  };

  const pasteButton = (setter: (v: string) => void) => (
    <button
      type="button"
      onClick={async () => {
        try {
          const text = (await readClipboardText()).trim();
          if (text) setter(text);
        } catch {
          /* clipboard empty / unreadable - no-op */
        }
      }}
      className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-cyber-text-secondary"
    >
      {t('model.paste')}
    </button>
  );

  const inputClass =
    'w-full bg-cyber-input border border-cyber-border px-2 py-1.5 pr-16 text-xs text-cyber-text font-mono focus:border-cyber-border focus:outline-none rounded-button';

  return (
    <div
      className="fixed inset-0 z-[9998] flex items-center justify-center"
      onKeyDown={(e) => {
        if (e.key === 'Escape') onClose();
      }}
    >
      <div className="absolute inset-0" onClick={onClose} />
      <div
        className="relative w-[450px] max-w-[90vw] border border-cyber-border/30 bg-cyber-surface shadow-2xl rounded-xl overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="h-px w-full bg-cyber-border" />
        <div className="px-6 pt-5 pb-4 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-cyber-text font-mono text-sm opacity-60">&gt;_</span>
            <span className="text-base font-bold text-cyber-text">{t('model.accessKey')}</span>
          </div>
          <button
            onClick={onClose}
            className="text-cyber-text-secondary close-button transition-colors"
          >
            <X size={18} />
          </button>
        </div>
        <div className="px-5 pb-5">
          <div className="space-y-4">
            {team && (
              <p className="text-xs text-cyber-text-secondary">{t('model.quota.teamHint')}</p>
            )}
            <div>
              <label className="block text-xs text-cyber-text-secondary mb-1">
                {team ? 'Organization ID' : t('model.akSkAccessKey')}
              </label>
              <div className="relative">
                <input
                  type="text"
                  placeholder={team ? 'Organization ID' : 'AK...'}
                  value={ak}
                  onChange={(e) => setAk(e.target.value)}
                  autoFocus
                  className={inputClass}
                />
                {pasteButton(setAk)}
              </div>
            </div>
            <div>
              <label className="block text-xs text-cyber-text-secondary mb-1">
                {team ? 'Project ID' : t('model.akSkSecretKey')}
              </label>
              <div className="relative">
                <input
                  type="text"
                  placeholder={team ? 'Project ID' : 'SK...'}
                  value={sk}
                  onChange={(e) => setSk(e.target.value)}
                  className={inputClass}
                />
                {pasteButton(setSk)}
              </div>
            </div>
          </div>
          <button
            type="button"
            onClick={() => shellOpen(accessUrl).catch(() => window.open(accessUrl, '_blank'))}
            className="block text-xs text-cyber-accent hover:opacity-80 pt-3"
          >
            {accessUrl}
          </button>
        </div>
        <div className="flex border-t border-cyber-border">
          <button
            type="button"
            onClick={onClose}
            className="flex-1 border-r border-cyber-border px-4 py-3 text-[14px] font-semibold text-cyber-text-secondary transition-all hover:bg-cyber-elevated hover:text-cyber-text"
          >
            {t('model.escCancel')}
          </button>
          <button
            type="button"
            onClick={handleSave}
            disabled={saving || !valid}
            aria-busy={saving || undefined}
            className="flex-1 px-4 py-3 text-[14px] font-semibold text-cyber-text transition-all enabled:hover:bg-cyber-text/10 disabled:text-cyber-text-muted"
          >
            {t('model.enterSave')}
          </button>
        </div>
      </div>
    </div>
  );
}
