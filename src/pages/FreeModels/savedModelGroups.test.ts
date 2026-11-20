import { describe, expect, it } from 'vitest';
import type { ModelConfig } from '../../api/types';
import { groupSavedModels, savedSiblings } from './savedModelGroups';

const model = (internalId: string, extra: Partial<ModelConfig> = {}): ModelConfig => ({
  internalId,
  name: 'Relay',
  baseUrl: 'https://relay.example/v1',
  apiKey: 'sk-one',
  modelId: internalId,
  ...extra,
});

describe('saved model groups', () => {
  it('merges model IDs saved with the same endpoint and key, in saved order', () => {
    const groups = groupSavedModels([
      model('a'),
      model('b', { apiKey: 'sk-two' }),
      model('c'),
      model('d', { baseUrl: 'https://other.example/v1' }),
    ]);
    expect(groups.map((group) => group.models.map((entry) => entry.internalId))).toEqual([
      ['a', 'c'],
      ['b'],
      ['d'],
    ]);
  });

  it('keeps account models and key-less entries on their own', () => {
    const account = { provider: 'codex', accountId: 'x' } as ModelConfig['accountSource'];
    const groups = groupSavedModels([
      model('a', { apiKey: '', accountSource: account }),
      model('b', { apiKey: '', accountSource: account }),
      model('c', { apiKey: '' }),
      model('d', { apiKey: '' }),
    ]);
    expect(groups).toHaveLength(4);
  });

  it('separates keys whose Anthropic endpoints differ', () => {
    expect(
      groupSavedModels([model('a'), model('b', { anthropicUrl: 'https://relay.example' })])
    ).toHaveLength(2);
  });

  it('lists the other models of the same key for the route card', () => {
    const models = [model('a'), model('b', { apiKey: 'sk-two' }), model('c')];
    expect(savedSiblings(models, 'a').map((entry) => entry.internalId)).toEqual(['c']);
    expect(savedSiblings(models, 'b')).toEqual([]);
    expect(savedSiblings(models, 'missing')).toEqual([]);
  });
});
