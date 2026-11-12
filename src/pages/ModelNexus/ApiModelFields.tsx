import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { readText as readClipboardText } from '@tauri-apps/plugin-clipboard-manager';
import * as api from '../../api/models';
import { ModelIdCombobox } from '../../components/ModelIdCombobox';
import { useToast } from '../../components/Toast';
import { useI18n } from '../../hooks/useI18n';
import modelDirectory from '../../data/modelDirectory.json';
import { isFreeModelOption } from '../../utils/modelOptions';
import type { NewModelForm } from './context';

function catalogInput(
  form: Pick<NewModelForm, 'baseUrl' | 'anthropicUrl' | 'apiKey'>
): api.UpstreamModelCatalogInput {
  return {
    baseUrl: form.baseUrl.trim(),
    anthropicUrl: form.anthropicUrl.trim(),
    apiKey: form.apiKey.trim(),
  };
}

function sameEndpoint(left: string, right: string): boolean {
  return left.replace(/\/+$/, '') === right.replace(/\/+$/, '');
}

export function ApiModelFields({
  form,
  setForm,
  editing = false,
  active = true,
}: {
  form: NewModelForm;
  setForm: React.Dispatch<React.SetStateAction<NewModelForm>>;
  editing?: boolean;
  active?: boolean;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const { baseUrl, anthropicUrl, apiKey } = form;
  const input = useMemo(
    () => catalogInput({ baseUrl, anthropicUrl, apiKey }),
    [baseUrl, anthropicUrl, apiKey]
  );
  const source = JSON.stringify(input);
  const latest = useRef({ source, active, t, showToast });
  useLayoutEffect(() => {
    latest.current = { source, active, t, showToast };
  }, [source, active, t, showToast]);
  const revision = useRef(0);
  const running = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [initial] = useState(() => ({
    baseUrl: input.baseUrl,
    anthropicUrl: input.anthropicUrl,
    options: form.modelIdOptions,
    freeModelIds: form.freeModelIds,
  }));
  const [pendingSource, setPendingSource] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<{
    source: string;
    catalog: api.UpstreamModelCatalog;
  } | null>(null);
  const catalog = loaded?.source === source ? loaded.catalog : undefined;
  const initialFreeIds =
    sameEndpoint(initial.baseUrl, input.baseUrl) &&
    sameEndpoint(initial.anthropicUrl, input.anthropicUrl)
      ? initial.freeModelIds
      : undefined;
  const validEndpoint = (() => {
    try {
      const url = new URL(input.baseUrl || input.anthropicUrl);
      return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password;
    } catch {
      return false;
    }
  })();
  const applyCatalog = useCallback(
    (next: api.UpstreamModelCatalog) => {
      if (!latest.current.active || latest.current.source !== source) return;
      setLoaded({ source, catalog: next });
      if ((next.requiresApiKey && (!input.apiKey || input.apiKey === 'not-needed')) || editing)
        return;
      setForm((current) => {
        if (JSON.stringify(catalogInput(current)) !== source || current.modelId) return current;
        const first =
          next.models.find((model) =>
            isFreeModelOption(model.id, {
              multiplier: model.multiplier,
              free:
                model.free ??
                (initialFreeIds?.includes(model.id) && !(model.multiplier! > 0) ? true : undefined),
            })
          ) ?? next.models[0];
        return first ? { ...current, modelId: first.id } : current;
      });
    },
    [source, setForm, editing, input.apiKey, initialFreeIds]
  );

  const refresh = useCallback(
    async (showErrors = true) => {
      if (!active || !validEndpoint || running.current === source) return;
      clearTimeout(timer.current);
      const attempt = ++revision.current;
      running.current = source;
      setPendingSource(source);
      try {
        const next = await api.fetchUpstreamModelCatalog(input);
        if (attempt === revision.current) applyCatalog(next);
      } catch (error) {
        if (
          attempt !== revision.current ||
          !latest.current.active ||
          latest.current.source !== source
        )
          return;
        const code = String(error);
        if (code === 'UPSTREAM_MODELS_UNSUPPORTED' && !editing) {
          const preset =
            sameEndpoint(initial.baseUrl, input.baseUrl) &&
            sameEndpoint(initial.anthropicUrl, input.anthropicUrl)
              ? initial.options?.[0]
              : undefined;
          if (preset)
            setForm((current) =>
              JSON.stringify(catalogInput(current)) === source && !current.modelId
                ? { ...current, modelId: preset }
                : current
            );
        }
        if (!showErrors) return;
        const { t: translate, showToast: toast } = latest.current;
        const message =
          code === 'UPSTREAM_MODELS_UNSUPPORTED'
            ? translate('model.catalogUnsupported')
            : code === 'UPSTREAM_MODELS_KEY_INVALID' ||
                /^UPSTREAM_MODELS_HTTP_(401|403)$/.test(code)
              ? translate('model.catalogKeyInvalid')
              : code === 'UPSTREAM_MODELS_INVALID_URL'
                ? translate('model.catalogInvalidUrl')
                : translate('model.catalogFailed');
        toast('error', message);
      } finally {
        if (attempt === revision.current) {
          running.current = null;
          setPendingSource(null);
        }
      }
    },
    [active, validEndpoint, source, input, applyCatalog, initial, editing, setForm]
  );

  useEffect(() => {
    const attempt = ++revision.current;
    running.current = null;
    if (!active || !validEndpoint) return;
    void api.getCachedUpstreamModelCatalog(input).then(
      (cached) => {
        if (attempt !== revision.current) return;
        setPendingSource(null);
        if (cached) applyCatalog(cached);
      },
      () => {
        if (attempt === revision.current) setPendingSource(null);
        // A cache miss or unreadable key must not interrupt opening an edit form.
      }
    );
    if (!editing) timer.current = setTimeout(() => void refresh(false), 800);
    return () => {
      clearTimeout(timer.current);
      revision.current += 1;
      running.current = null;
    };
  }, [source, input, active, editing, validEndpoint, applyCatalog, refresh]);

  const fallback = (
    [...modelDirectory.providers, ...modelDirectory.relays] as api.ModelDirectoryEntry[]
  ).find(
    (entry) =>
      (input.baseUrl || input.anthropicUrl) &&
      sameEndpoint(entry.baseUrl, input.baseUrl) &&
      sameEndpoint(entry.anthropicUrl, input.anthropicUrl)
  );
  const initialOptions =
    sameEndpoint(initial.baseUrl, input.baseUrl) &&
    sameEndpoint(initial.anthropicUrl, input.anthropicUrl)
      ? initial.options
      : undefined;
  const needsKey =
    (!input.apiKey || input.apiKey === 'not-needed') &&
    (catalog?.requiresApiKey ?? (validEndpoint && !input.apiKey));
  const options = needsKey
    ? (initialOptions ?? fallback?.modelIds ?? [])
    : (catalog?.models.map((model) => model.id) ?? initialOptions ?? fallback?.modelIds ?? []);
  const freeIds = new Set(catalog?.models.filter((model) => model.free).map((model) => model.id));
  for (const id of initialFreeIds ?? []) {
    const model = catalog?.models.find((model) => model.id === id);
    if (
      model?.free !== false &&
      !(model?.free !== true && model?.multiplier !== undefined && model.multiplier > 0)
    )
      freeIds.add(id);
  }

  return (
    <div>
      <label className="block text-xs text-cyber-text-secondary mb-1">{t('model.modelId')}</label>
      <ModelIdCombobox
        inlineActions
        ariaLabel={t('model.modelId')}
        value={form.modelId}
        onChange={(value) => setForm((current) => ({ ...current, modelId: value }))}
        options={options}
        freeModelIds={catalog || initialFreeIds?.length ? [...freeIds] : undefined}
        paidModelIds={catalog?.models
          .filter((model) => model.free === false)
          .map((model) => model.id)}
        freeLabel={t('model.free')}
        optionMultipliers={Object.fromEntries(
          catalog?.models
            .filter((model) => model.multiplier !== undefined)
            .map((model) => [model.id, model.multiplier!]) ?? []
        )}
        disabled={!active}
        actionHint={needsKey ? t('model.catalogKeyRequired') : undefined}
        placeholder={t('model.modelIdPlaceholder')}
        onRefresh={validEndpoint ? () => void refresh() : undefined}
        refreshLabel={t('btn.refresh')}
        refreshing={pendingSource === source}
        onPaste={async () => {
          try {
            const text = (await readClipboardText()).trim();
            if (text && latest.current.active && latest.current.source === source)
              setForm((current) => ({ ...current, modelId: text }));
          } catch {
            // Clipboard empty or unreadable.
          }
        }}
        pasteLabel={t('model.paste')}
      />
    </div>
  );
}
