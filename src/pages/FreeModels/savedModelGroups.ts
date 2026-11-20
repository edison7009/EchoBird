import type { ModelConfig } from '../../api/types';

/**
 * Saved models that share one key: the same endpoint and the same API key,
 * saved once per model ID. Smart access lists them as one card, and the
 * route card switches between their model IDs.
 */
export interface SavedKeyGroup {
  key: string;
  models: ModelConfig[];
}

type KeyFields = Pick<ModelConfig, 'internalId' | 'baseUrl' | 'anthropicUrl'> & {
  apiKey?: string;
  accountSource?: ModelConfig['accountSource'];
};

/** Account models and key-less entries stand alone; the rest group by endpoint and key. */
export function savedKeyOf(model: KeyFields): string {
  if (model.accountSource || !model.apiKey) return `model:${model.internalId}`;
  return ['key', model.baseUrl, model.anthropicUrl ?? '', model.apiKey].join('\n');
}

/**
 * The saved models sharing a route card's key, including itself when it is
 * one: a smart-router copy of a saved key finds its originals this way.
 */
export function sameKeyModels(models: readonly ModelConfig[], model: KeyFields): ModelConfig[] {
  const key = savedKeyOf(model);
  return models.filter(
    (entry) => entry.internalId === model.internalId || savedKeyOf(entry) === key
  );
}

/** Groups in first-seen order; each group keeps its models in their saved order. */
export function groupSavedModels(models: readonly ModelConfig[]): SavedKeyGroup[] {
  const groups = new Map<string, ModelConfig[]>();
  for (const model of models) {
    const key = savedKeyOf(model);
    const group = groups.get(key);
    if (group) group.push(model);
    else groups.set(key, [model]);
  }
  return [...groups].map(([key, members]) => ({ key, models: members }));
}

/** The other saved models of the same key as `id`, for switching the route card between them. */
export function savedSiblings(models: readonly ModelConfig[], id: string): ModelConfig[] {
  const own = models.find((model) => model.internalId === id);
  if (!own) return [];
  const key = savedKeyOf(own);
  return models.filter((model) => model.internalId !== id && savedKeyOf(model) === key);
}
