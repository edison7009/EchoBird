import type { ModelConfig } from '../api/types';

type Loader = () => Promise<ModelConfig[]>;
type Update = ModelConfig[] | ((models: ModelConfig[]) => ModelConfig[]);
interface Snapshot {
  models: ModelConfig[];
  entities: Record<string, ModelConfig>;
  removedIds: ReadonlySet<string>;
  loaded: boolean;
  loading: boolean;
  revision: number;
}

export function createModelRepository() {
  const empty = (): Snapshot => ({
    models: [],
    entities: {},
    removedIds: new Set(),
    loaded: false,
    loading: false,
    revision: 0,
  });
  let state = empty();
  let epoch = 0;
  let pending: Promise<ModelConfig[]> | undefined;
  let loader: Loader | undefined;
  let orderPending = false;
  let stale = false;
  const entityRevisions = new Map<string, number>();
  const listeners = new Set<() => void>();
  const publish = (change: Partial<Snapshot>) => {
    state = { ...state, ...change, revision: state.revision + 1 };
    listeners.forEach((listener) => listener());
  };
  const repository = {
    getSnapshot: () => state,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
        queueMicrotask(() => {
          if (!listeners.size) repository.reset();
        });
      };
    },
    async load(read: Loader, force = false): Promise<ModelConfig[]> {
      loader = read;
      if (!force && pending) return pending;
      if (!force && state.loaded && !stale) return state.models;
      const request = ++epoch;
      const readRevision = state.revision;
      publish({ loading: true });
      const operation = Promise.resolve()
        .then(read)
        .then((models) => {
          if (request !== epoch) return state.models;
          const entities = { ...state.entities };
          const removedIds = new Set(state.removedIds);
          for (const old of state.models) {
            if ((entityRevisions.get(old.internalId) ?? 0) <= readRevision) {
              delete entities[old.internalId];
            }
          }
          for (const model of models) {
            if ((entityRevisions.get(model.internalId) ?? 0) <= readRevision) {
              entities[model.internalId] = model;
              removedIds.delete(model.internalId);
            }
          }
          const listed = models.flatMap((model) =>
            entities[model.internalId] ? [entities[model.internalId]] : []
          );
          stale = false;
          publish({ models: listed, entities, removedIds, loaded: true, loading: false });
          return listed;
        })
        .finally(() => {
          if (request === epoch) {
            pending = undefined;
            if (state.loading) publish({ loading: false });
          }
        });
      pending = operation;
      return operation;
    },
    invalidate() {
      epoch++;
      pending = undefined;
      stale = true;
      publish({ loading: false });
      if (loader && listeners.size) void repository.load(loader, true).catch(() => {});
    },
    ingest(models: ModelConfig[], readRevision = state.revision) {
      const entities = { ...state.entities };
      const removedIds = new Set(state.removedIds);
      for (const model of models) {
        if ((entityRevisions.get(model.internalId) ?? 0) > readRevision) continue;
        entities[model.internalId] = model;
        entityRevisions.set(model.internalId, state.revision + 1);
        removedIds.delete(model.internalId);
      }
      publish({
        entities,
        removedIds,
        models: state.models.map((model) => entities[model.internalId]),
      });
    },
    remove(id: string) {
      epoch++;
      pending = undefined;
      const entities = { ...state.entities };
      delete entities[id];
      entityRevisions.set(id, state.revision + 1);
      publish({
        loading: false,
        entities,
        removedIds: new Set([...state.removedIds, id]),
        models: state.models.filter((model) => model.internalId !== id),
      });
    },
    setView(filter: (model: ModelConfig) => boolean, update: Update) {
      epoch++;
      pending = undefined;
      const next = typeof update === 'function' ? update(state.models.filter(filter)) : update;
      const models = [...state.models.filter((model) => !filter(model)), ...next];
      publish({
        loading: false,
        models,
        entities: {
          ...state.entities,
          ...Object.fromEntries(next.map((model) => [model.internalId, model])),
        },
      });
    },
    async reorder(ids: string[], write: () => Promise<boolean>) {
      if (orderPending) throw new Error('A model order update is already pending');
      orderPending = true;
      const before = state.models.map((model) => model.internalId);
      const applyOrder = (order: string[]) => {
        const rank = new Map(order.map((id, index) => [id, index]));
        const selected = state.models.filter((model) => rank.has(model.internalId));
        selected.sort((a, b) => rank.get(a.internalId)! - rank.get(b.internalId)!);
        let index = 0;
        epoch++;
        pending = undefined;
        publish({
          loading: false,
          models: state.models.map((model) =>
            rank.has(model.internalId) ? selected[index++] : model
          ),
        });
      };
      applyOrder(ids);
      try {
        if (!(await write())) throw new Error('Model order could not be saved');
        // A focus refresh during the write may have read the previous disk order.
        applyOrder(ids);
        return true;
      } catch (error) {
        // Restore order using current entities so concurrent edits/deletions survive.
        applyOrder(before);
        throw error;
      } finally {
        orderPending = false;
      }
    },
    reset() {
      epoch++;
      pending = undefined;
      loader = undefined;
      stale = false;
      entityRevisions.clear();
      state = empty();
    },
  };
  return repository;
}

export const modelRepository = createModelRepository();
