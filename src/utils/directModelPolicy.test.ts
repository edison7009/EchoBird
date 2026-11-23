import { describe, expect, it } from 'vitest';
import type { ModelConfig } from '../api/types';
import { directModelProtocol } from './directModelPolicy';

const model: ModelConfig = {
  internalId: 'imported',
  name: 'Imported',
  modelId: 'vendor/model',
  baseUrl: 'https://test.example/v1',
  apiKey: 'fixture',
  openaiResponses: true,
};

describe('direct model protocol selection', () => {
  it('offers Responses only for tools that can apply it', () => {
    expect(directModelProtocol(model, 'codex', ['openai'])).toBe('openai-responses');
    expect(directModelProtocol(model, 'vscode', ['openai', 'anthropic'])).toBe('openai-responses');
    expect(directModelProtocol(model, 'opencode', ['openai', 'anthropic'])).toBeUndefined();
  });

  it('keeps a Chat Completions endpoint out of Responses-only apps', () => {
    const gemini = { ...model, modelId: 'gemini-fixture', openaiResponses: false };
    for (const tool of ['codex', 'chatgptdesktop']) {
      expect(directModelProtocol(gemini, tool, ['openai'])).toBeUndefined();
      expect(directModelProtocol({ ...gemini, openaiResponses: true }, tool, ['openai'])).toBe(
        'openai-responses'
      );
    }
    expect(directModelProtocol(gemini, 'opencode', ['openai'])).toBe('openai');
    for (const tool of ['claudecode', 'claudedesktop']) {
      expect(directModelProtocol(gemini, tool, ['anthropic'])).toBeUndefined();
      expect(
        directModelProtocol({ ...gemini, anthropicUrl: 'https://relay.example/anthropic' }, tool, [
          'anthropic',
        ])
      ).toBe('anthropic');
    }
  });

  it('follows the available endpoints and tool protocol order', () => {
    const both = { ...model, anthropicUrl: 'https://test.example/anthropic' };
    expect(directModelProtocol(both, 'opencode', ['openai', 'anthropic'])).toBe('anthropic');
    expect(directModelProtocol(both, 'vscode', ['anthropic', 'openai'])).toBe('anthropic');
    expect(
      directModelProtocol({ ...model, openaiResponses: undefined }, 'opencode', ['openai'])
    ).toBe('openai');
  });
});
