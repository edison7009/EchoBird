import { beforeEach, expect, it, vi } from 'vitest';
import * as voiceApi from '../../api/voiceInput';
import { useVoiceInputStore as voice } from '../../stores/voiceInputStore';
import { voiceSnapshot } from './testFixtures';
import { saveVoiceSource } from './sources';
import type { NewModelForm } from '../ModelNexus/context';

vi.mock('../../api/voiceInput', () => ({
  addVoiceApiSource: vi.fn(),
  saveVoiceConfig: vi.fn(),
}));
const form: NewModelForm = {
  name: 'MiMo ASR',
  baseUrl: 'https://api.xiaomimimo.com/v1',
  anthropicUrl: '',
  apiKey: 'fixture-key',
  modelId: 'mimo-v2.5-asr',
  voiceOptions: { protocol: 'mimo' },
};
const saved = {
  internalId: 'voice-model',
  name: form.name,
  baseUrl: form.baseUrl,
  apiKey: '',
  modelId: form.modelId,
  scope: 'voiceInput' as const,
};
const attached = voiceSnapshot({
  config: {
    ...voiceSnapshot().config,
    channels: [{ kind: 'model', id: saved.internalId }],
    apiOptions: { [saved.internalId]: { protocol: 'mimo' } },
    shortcutEnabled: true,
  },
  revision: 2,
});
beforeEach(() => {
  vi.resetAllMocks();
  voice.setState({ ...voice.getInitialState(), snapshot: voiceSnapshot() });
  vi.mocked(voiceApi.addVoiceApiSource).mockResolvedValue([attached, saved]);
  vi.mocked(voiceApi.saveVoiceConfig).mockImplementation(async (config) =>
    voiceSnapshot({ config, revision: 2 })
  );
});
it('saves a voice-scoped model with its matching protocol and automatically enables voice input', async () => {
  await saveVoiceSource(form);
  expect(voiceApi.addVoiceApiSource).toHaveBeenCalledExactlyOnceWith({
    name: form.name,
    baseUrl: form.baseUrl,
    apiKey: form.apiKey,
    modelId: form.modelId,
    options: form.voiceOptions,
  });
  expect(voice.getState().models).toEqual([saved]);
  expect(voice.getState().snapshot?.config).toMatchObject({
    channels: [{ kind: 'model', id: saved.internalId }],
    apiOptions: { [saved.internalId]: { protocol: 'mimo' } },
    shortcutEnabled: true,
  });
});
it('uses the saved account speech ID and only writes a channel, without creating an API or chat model', async () => {
  const accountId = 'a'.repeat(64);
  await saveVoiceSource({
    ...form,
    accountSource: { provider: 'workBuddy', accountId },
    modelId: `workbuddy:${accountId}`,
  });
  expect(voiceApi.addVoiceApiSource).not.toHaveBeenCalled();
  expect(voice.getState().snapshot?.config.channels).toEqual([
    { kind: 'workbuddy', id: accountId },
  ]);
  await saveVoiceSource({
    ...form,
    accountSource: { provider: 'workBuddy', accountId },
    modelId: `workbuddy:${accountId}`,
  });
  expect(voiceApi.saveVoiceConfig).toHaveBeenCalledTimes(1);
});
it('preserves previous channels and unlocks the controls when adding an API source fails', async () => {
  vi.mocked(voiceApi.addVoiceApiSource).mockRejectedValue('voice.error.save');
  await expect(saveVoiceSource(form)).rejects.toBe('voice.error.save');
  expect(voice.getState().saving).toBe(false);
  expect(voice.getState().snapshot?.config.channels).toEqual([]);
  expect(voice.getState().models).toEqual([]);
});

it.each([
  ['workBuddyAi', 'workbuddyai'],
  ['claudeCode', 'claudecode'],
  ['codex', 'codex'],
  ['minimax', 'minimax'],
  ['antigravity', 'antigravity'],
  ['cursor', 'cursor'],
  ['grokbot', 'grokbot'],
  ['dumate', 'dumate'],
  ['doubaowork', 'doubaowork'],
  ['devin', 'devin'],
  ['trae', 'trae'],
  ['codeArts', 'codearts'],
  ['traecn', 'traecn'],
  ['qoder', 'qoder'],
  ['qoderCn', 'qodercn'],
  ['qwenworkcn', 'qwenworkcn'],
  ['qwenwork', 'qwenwork'],
  ['kilo', 'kilo'],
] as const)(
  'attaches %s speech by reference and rejects a mismatched account before writes',
  async (provider, prefix) => {
    const hash = 'a'.repeat(64);
    const accountId = provider === 'codeArts' ? `codearts:${hash}` : hash;
    const modelId = `${prefix}:${hash}${provider === 'kilo' ? ':vendor:speech-model' : ''}`;
    await saveVoiceSource({ ...form, accountSource: { provider, accountId }, modelId });
    expect(voice.getState().snapshot?.config.channels).toEqual([{ kind: 'account', id: modelId }]);
    expect(voiceApi.addVoiceApiSource).not.toHaveBeenCalled();
    await saveVoiceSource({ ...form, accountSource: { provider, accountId }, modelId });
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledTimes(1);
    await expect(
      saveVoiceSource({ ...form, accountSource: { provider, accountId: 'b'.repeat(64) }, modelId })
    ).rejects.toThrow('voice.error.missingSource');
    expect(voiceApi.saveVoiceConfig).toHaveBeenCalledTimes(1);
  }
);
it('does not submit a dismissed form', async () => {
  await saveVoiceSource(form, () => false);
  expect(voiceApi.addVoiceApiSource).not.toHaveBeenCalled();
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
});
it('locks channel changes during API source creation and applies the committed result once', async () => {
  let resolve!: (result: [typeof attached, typeof saved]) => void;
  vi.mocked(voiceApi.addVoiceApiSource).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    })
  );
  const pending = saveVoiceSource(form);
  await expect(saveVoiceSource(form)).rejects.toThrow('voice.error.busy');
  expect(await voice.getState().save({ channels: [] })).toBe(false);
  resolve([attached, saved]);
  await pending;
  expect(voiceApi.addVoiceApiSource).toHaveBeenCalledTimes(1);
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  expect(voice.getState().models).toEqual([saved]);
  expect(voice.getState().saving).toBe(false);
});
it.each(['recording', 'testing', 'transcribing'] as const)(
  'does not write or create models while %s',
  async (phase) => {
    voice.setState({ snapshot: voiceSnapshot({ phase }) });
    await expect(saveVoiceSource(form)).rejects.toThrow('voice.error.busy');
    expect(voiceApi.addVoiceApiSource).not.toHaveBeenCalled();
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  }
);
it.each(['api', 'account'] as const)(
  'rejects a fourth %s source on Save without creating or saving anything',
  async (kind) => {
    voice.setState({
      snapshot: voiceSnapshot({
        config: {
          ...voiceSnapshot().config,
          channels: [1, 2, 3].map((id) => ({ kind: 'model', id: String(id) })),
        },
      }),
    });
    await expect(
      saveVoiceSource(
        kind === 'account'
          ? {
              ...form,
              accountSource: { provider: 'workBuddy', accountId: 'a'.repeat(64) },
              modelId: `workbuddy:${'a'.repeat(64)}`,
            }
          : form
      )
    ).rejects.toThrow('voice.error.channels');
    expect(voiceApi.addVoiceApiSource).not.toHaveBeenCalled();
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  }
);
it('rejects a non-speech account model without creating or saving anything', async () => {
  await expect(
    saveVoiceSource({
      ...form,
      accountSource: { provider: 'workBuddy', accountId: 'a'.repeat(64) },
      modelId: 'chat-model',
    })
  ).rejects.toThrow('voice.error.missingSource');
  expect(voiceApi.addVoiceApiSource).not.toHaveBeenCalled();
  expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
});
it.each(['', 'file:///tmp/audio', 'https://user:secret@example.com/v1'])(
  'rejects unusable speech addresses before storing credentials: %s',
  async (baseUrl) => {
    await expect(saveVoiceSource({ ...form, baseUrl })).rejects.toThrow('voice.error.speechApi');
    expect(voiceApi.addVoiceApiSource).not.toHaveBeenCalled();
    expect(voiceApi.saveVoiceConfig).not.toHaveBeenCalled();
  }
);
