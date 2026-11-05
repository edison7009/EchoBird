import { expect, it, vi } from 'vitest';
import { createModelRepository } from './modelRepository';
import type { ModelConfig } from '../api/types';
const model = (name: string): ModelConfig => ({
  internalId: name,
  name,
  baseUrl: 'https://fixture.invalid',
  apiKey: '',
});
it('deduplicates reads, preserves rows on failures and ignores late snapshots after a mutation', async () => {
  const repo = createModelRepository();
  let resolve!: (value: ModelConfig[]) => void;
  const read = vi.fn(
    () =>
      new Promise<ModelConfig[]>((done) => {
        resolve = done;
      })
  );
  const first = repo.load(read);
  const second = repo.load(read);
  await Promise.resolve();
  expect(read).toHaveBeenCalledTimes(1);
  resolve([model('saved')]);
  await Promise.all([first, second]);
  const late = repo.load(read, true);
  await Promise.resolve();
  repo.setView(() => true, [model('newer')]);
  resolve([model('stale')]);
  await late;
  expect(repo.getSnapshot().models.map((m) => m.name)).toEqual(['newer']);
  await expect(
    repo.load(async () => {
      throw Error('offline');
    }, true)
  ).rejects.toThrow('offline');
  expect(repo.getSnapshot().models.map((m) => m.name)).toEqual(['newer']);
  repo.ingest([{ ...model('route'), scope: 'smartRouter' }]);
  expect(repo.getSnapshot().entities.route.scope).toBe('smartRouter');
  expect(repo.getSnapshot().models).toHaveLength(1);
});

it('rolls back failed orders without resurrecting a concurrently removed model', async () => {
  const repo = createModelRepository();
  repo.setView(() => true, [model('a'), model('b'), model('c')]);
  let fail!: (error: Error) => void;
  const write = () =>
    new Promise<boolean>((_, reject) => {
      fail = reject;
    });
  const saving = repo.reorder(['c', 'b', 'a'], write);
  expect(repo.getSnapshot().models.map((m) => m.name)).toEqual(['c', 'b', 'a']);
  await expect(repo.reorder(['a', 'b', 'c'], write)).rejects.toThrow('pending');
  repo.remove('b');
  repo.ingest([{ ...model('a'), name: 'edited' }]);
  fail(Error('disk full'));
  await expect(saving).rejects.toThrow('disk full');
  expect(repo.getSnapshot().models.map((m) => m.name)).toEqual(['edited', 'c']);
});

it('keeps a committed order when a focus refresh read the old order during the write', async () => {
  const repo = createModelRepository();
  await repo.load(async () => [model('a'), model('b')]);
  let commit!: (saved: boolean) => void;
  const saving = repo.reorder(
    ['b', 'a'],
    () =>
      new Promise<boolean>((done) => {
        commit = done;
      })
  );
  let resolveRead!: (models: ModelConfig[]) => void;
  const refresh = repo.load(
    () =>
      new Promise<ModelConfig[]>((done) => {
        resolveRead = done;
      }),
    true
  );
  await Promise.resolve();
  commit(true);
  await saving;
  resolveRead([model('a'), model('b')]);
  await refresh;
  expect(repo.getSnapshot().models.map((m) => m.name)).toEqual(['b', 'a']);
});

it('keeps committed entity updates when an earlier list read completes', async () => {
  const repo = createModelRepository();
  let resolve!: (models: ModelConfig[]) => void;
  const loading = repo.load(
    () =>
      new Promise<ModelConfig[]>((done) => {
        resolve = done;
      })
  );
  await Promise.resolve();
  repo.ingest([{ ...model('saved'), name: 'updated' }]);
  resolve([model('saved')]);
  await loading;
  expect(repo.getSnapshot().models[0].name).toBe('updated');
  expect(repo.getSnapshot().entities.saved.name).toBe('updated');
});

it('ignores stale route snapshots after edits and deletion but accepts fresh recreation', () => {
  const repo = createModelRepository();
  repo.ingest([model('saved')]);
  const revision = repo.getSnapshot().revision;
  repo.ingest([{ ...model('saved'), name: 'updated' }]);
  repo.ingest([model('saved')], revision);
  expect(repo.getSnapshot().entities.saved.name).toBe('updated');
  repo.remove('saved');
  repo.ingest([model('saved')], revision);
  expect(repo.getSnapshot().entities.saved).toBeUndefined();
  expect(repo.getSnapshot().removedIds.has('saved')).toBe(true);
  repo.ingest([model('saved')]);
  expect(repo.getSnapshot().removedIds.has('saved')).toBe(false);
});

it('accepts restored entities from a fresh authoritative list read', async () => {
  const repo = createModelRepository();
  repo.ingest([model('saved')]);
  repo.remove('saved');
  await repo.load(async () => [model('saved')]);
  expect(repo.getSnapshot().entities.saved.name).toBe('saved');
  expect(repo.getSnapshot().removedIds.has('saved')).toBe(false);
});
