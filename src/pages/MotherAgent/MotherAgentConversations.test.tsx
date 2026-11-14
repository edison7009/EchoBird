import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import type { AgentEvent, ModelConfig, ParasiteEvent, ReasoningEffort } from '../../api/types';
import { useNavigationStore } from '../../stores/navigationStore';
import { MotherAgentProvider } from './MotherAgentProvider';
import { useMotherAgent } from './context';
import type { MotherAgentCtx } from './types';
import { REASONING_STORAGE_KEY } from './reasoning';
import { listen } from '@tauri-apps/api/event';
import { loadLocale, translate } from '../../i18n';

const { selectionLabels } = vi.hoisted(() => ({ selectionLabels: new Map<string, string>() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

vi.mock('../../api/tauri', () => ({
  getModels: vi.fn(),
  loadSSHServers: vi.fn(),
  saveSSHServer: vi.fn(),
  removeSSHServerFromDisk: vi.fn(),
  parasiteListInstalled: vi.fn(),
  listenAgentEvents: vi.fn(),
  listenParasiteEvents: vi.fn(),
  sendAgentMessage: vi.fn(),
  abortAgent: vi.fn(),
  resetAgent: vi.fn(),
  resetConversationContext: vi.fn(),
  parasiteSendMessage: vi.fn(),
  parasiteAbort: vi.fn(),
  parasiteReset: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => selectionLabels.get(key) ?? key, locale: 'en' }),
}));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../FreeModels/provider', () => ({ useFreeModels: () => ({ routerEnabled }) }));

const confirm = vi.fn();
const showToast = vi.fn();

const model: ModelConfig = {
  internalId: 'test-model',
  name: 'Test model',
  modelId: 'test-model-api',
  baseUrl: 'https://example.test/v1',
  apiKey: 'fixture-key',
};
let state: MotherAgentCtx;
let renderer: ReactTestRenderer | undefined;
let agentEvent: (event: AgentEvent) => void;
let parasiteEvent: (event: ParasiteEvent) => void;
let stored: Map<string, string>;
let routerEnabled = true;
let selectionQuestion: (event: {
  payload: { source: string; error: 'voice.selectionEmpty' | 'voice.selectionTooLong' | null };
}) => void;

function Harness() {
  const result = useMotherAgent();
  useLayoutEffect(() => {
    state = result;
  });
  return null;
}

async function mount() {
  await act(async () => {
    renderer = create(
      <MotherAgentProvider>
        <Harness />
      </MotherAgentProvider>
    );
  });
}

function activeSession() {
  return state.conversations.find((c) => c.id === state.activeConversationId)!.sessionKey;
}

async function send(text: string) {
  await act(async () => {
    state.sendMessage(text);
  });
}

function emitAgent(event: AgentEvent) {
  const request = vi.mocked(api.sendAgentMessage).mock.lastCall?.[0];
  act(() => agentEvent({ ...event, request_id: event.request_id ?? request?.request_id }));
}

function emitParasite(event: ParasiteEvent) {
  const request = vi.mocked(api.parasiteSendMessage).mock.lastCall?.[0];
  act(() => parasiteEvent({ ...event, request_id: event.request_id ?? request?.requestId }));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-12T09:00:00Z'));
  vi.resetAllMocks();
  selectionLabels.clear();
  vi.mocked(listen).mockImplementation(async (name, callback) => {
    if (name === 'voice-selection-ask') selectionQuestion = callback as typeof selectionQuestion;
    return vi.fn<() => void>();
  });
  routerEnabled = true;
  confirm.mockResolvedValue(true);
  stored = new Map([['echobird_agent_model', model.internalId]]);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
    key: (index: number) => [...stored.keys()][index] ?? null,
    get length() {
      return stored.size;
    },
  });
  vi.stubGlobal('window', new EventTarget());
  let sequence = 0;
  vi.stubGlobal('crypto', { randomUUID: () => `conversation-${++sequence}` });
  useNavigationStore.setState({
    activePage: 'mother',
    motherPrefill: undefined,
    motherPrefillCursor: undefined,
    agentRunning: false,
  });
  vi.mocked(api.getModels).mockResolvedValue([model]);
  vi.mocked(api.loadSSHServers).mockResolvedValue([
    { id: 'remote', host: '192.0.2.1', port: 22, username: 'tester', password: '' },
  ]);
  vi.mocked(api.parasiteListInstalled).mockResolvedValue(['claudecode']);
  vi.mocked(api.listenAgentEvents).mockImplementation(async (handler) => {
    agentEvent = handler;
    return vi.fn<() => void>();
  });
  vi.mocked(api.listenParasiteEvents).mockImplementation(async (handler) => {
    parasiteEvent = handler;
    return vi.fn<() => void>();
  });
  vi.mocked(api.sendAgentMessage).mockResolvedValue('ok');
  vi.mocked(api.abortAgent).mockResolvedValue(true);
  vi.mocked(api.resetAgent).mockResolvedValue('ok');
  vi.mocked(api.resetConversationContext).mockResolvedValue('ok');
  vi.mocked(api.parasiteSendMessage).mockResolvedValue(undefined);
  vi.mocked(api.parasiteAbort).mockResolvedValue(true);
  vi.mocked(api.parasiteReset).mockResolvedValue(undefined);
});

afterEach(() => {
  if (renderer) act(() => renderer?.unmount());
  renderer = undefined;
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it.each(['zh-Hans', 'zh-Hant', 'en', 'ja'])(
  'prefills a %s selection question once without sending it or resetting the conversation',
  async (locale) => {
    await loadLocale(locale);
    const question = translate('mother.selectionQuestion', locale);
    const content = translate('mother.selectionContent', locale);
    selectionLabels.set('mother.selectionQuestion', question);
    selectionLabels.set('mother.selectionContent', content);
    await mount();
    const session = state.activeConversationId;
    const source = 'Selected text 😀\n<script>plain text</script>';
    act(() => selectionQuestion({ payload: { source, error: null } }));
    const draft = `${question}\n${content}${source}`;
    expect(useNavigationStore.getState().activePage).toBe('mother');
    expect(state.chatInput).toBe(draft);
    expect(state.chatPrefillRef.current).toEqual({ text: draft, cursor: question.length });
    expect(useNavigationStore.getState().motherPrefill).toBeUndefined();
    expect(state.activeConversationId).toBe(session);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(api.parasiteSendMessage).not.toHaveBeenCalled();
  }
);

it('retains the existing application-install prefill with its caret at the end', async () => {
  await mount();
  const text = 'Install selected application';
  act(() => useNavigationStore.getState().goToMother(text));
  expect(state.chatInput).toBe(text);
  expect(state.chatPrefillRef.current).toEqual({ text, cursor: text.length });
  expect(api.sendAgentMessage).not.toHaveBeenCalled();
});

it('preserves an edited question through back and forward without replaying or sending its prefill', async () => {
  await mount();
  act(() => useNavigationStore.getState().setActivePage('voice'));
  act(() => useNavigationStore.getState().goToMother('Question: \nSelected text: original', 10));
  const edited = 'Question: explain this\nSelected text: original';
  act(() => state.setChatInput(edited));
  act(() => useNavigationStore.getState().goBack());
  expect(useNavigationStore.getState().activePage).toBe('voice');
  expect(state.chatInput).toBe(edited);
  act(() => useNavigationStore.getState().goForward());
  expect(useNavigationStore.getState().activePage).toBe('mother');
  expect(state.chatInput).toBe(edited);
  expect(useNavigationStore.getState().motherPrefill).toBeUndefined();
  expect(api.sendAgentMessage).not.toHaveBeenCalled();
  expect(api.parasiteSendMessage).not.toHaveBeenCalled();
});

it('preserves the draft and navigation if selection capture fails', async () => {
  await mount();
  act(() => state.setChatInput('Keep existing draft'));
  act(() => useNavigationStore.getState().setActivePage('voice'));
  act(() => selectionQuestion({ payload: { source: '', error: 'voice.selectionEmpty' } }));
  expect(state.chatInput).toBe('Keep existing draft');
  expect(useNavigationStore.getState().activePage).toBe('voice');
  expect(showToast).toHaveBeenCalledWith('error', 'voice.selectionEmpty');
  expect(api.sendAgentMessage).not.toHaveBeenCalled();
  expect(api.parasiteSendMessage).not.toHaveBeenCalled();
});

it('ignores a queued selection event after the assistant provider unmounts', async () => {
  await mount();
  const callback = selectionQuestion;
  act(() => renderer!.unmount());
  renderer = undefined;
  act(() => useNavigationStore.getState().setActivePage('voice'));
  act(() => callback({ payload: { source: 'Stale selected text', error: null } }));
  expect(useNavigationStore.getState().activePage).toBe('voice');
  expect(api.sendAgentMessage).not.toHaveBeenCalled();
});

describe('Mother Agent reasoning workflow', () => {
  const deepseek = { ...model, internalId: 'deepseek', modelId: 'deepseek-flash' };
  const gpt = { ...model, internalId: 'gpt', modelId: 'gpt-5.2' };
  const anthropic = {
    ...model,
    internalId: 'anthropic',
    modelId: 'custom-messages-model',
    baseUrl: '',
    anthropicUrl: 'https://example.test/anthropic',
  };
  const router = { ...model, internalId: 'router', scope: 'smartRouter' as const };
  const gateway = { ...model, internalId: 'gateway', baseUrl: 'https://openrouter.ai/api/v1' };
  const preferenceKey = (entry: ModelConfig) =>
    JSON.stringify([
      entry.internalId,
      entry.modelId || entry.name,
      entry.baseUrl,
      entry.anthropicUrl || '',
    ]);
  beforeEach(() => {
    stored.set('echobird_agent_model', deepseek.internalId);
    vi.mocked(api.getModels).mockResolvedValue([deepseek, gpt, model, anthropic, router, gateway]);
  });

  it('starts with default and sends no extra parameter without an explicit choice', async () => {
    await mount();
    expect(state.reasoningEffort).toBeUndefined();
    expect(state.reasoningLevels).toEqual(['low', 'medium', 'high', 'xhigh', 'max']);
    expect(stored.has(REASONING_STORAGE_KEY)).toBe(false);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    await send('Use model default');
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0]).not.toHaveProperty(
      'reasoning_effort'
    );
  });

  it('remembers each model separately without model requests, native settings changes, or dialogs', async () => {
    await mount();
    act(() => state.setReasoningEffort('max'));
    act(() => state.setAgentModel(gpt.internalId));
    expect(state.reasoningEffort).toBeUndefined();
    act(() => state.setReasoningEffort('xhigh'));
    act(() => state.setAgentModel(deepseek.internalId));
    expect(state.reasoningEffort).toBe('max');
    act(() => state.setAgentModel(gpt.internalId));
    expect(state.reasoningEffort).toBe('xhigh');
    act(() => useNavigationStore.setState({ activePage: 'apps' }));
    act(() => useNavigationStore.setState({ activePage: 'mother' }));
    act(() => state.createConversation());
    expect(state.reasoningEffort).toBe('xhigh');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(api.parasiteSendMessage).not.toHaveBeenCalled();
    expect(api.saveSSHServer).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    act(() => renderer?.unmount());
    renderer = undefined;
    await mount();
    expect(state.reasoningEffort).toBe('xhigh');
  });

  it('keeps all five choices for every API model and sends only on explicit turns', async () => {
    await mount();
    const levels: ReasoningEffort[] = ['low', 'medium', 'high', 'xhigh', 'max'];
    for (const selected of [deepseek, gpt, model, anthropic, router, gateway]) {
      act(() => state.setAgentModel(selected.internalId));
      expect(state.reasoningLevels).toEqual(levels);
      for (const effort of levels) {
        const requestCount = vi.mocked(api.sendAgentMessage).mock.calls.length;
        act(() => state.setReasoningEffort(effort));
        expect(state.reasoningEffort).toBe(effort);
        expect(api.sendAgentMessage).toHaveBeenCalledTimes(requestCount);
        await send(`Use ${effort}`);
        expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0].reasoning_effort).toBe(effort);
        emitAgent({ type: 'done', conversation_id: activeSession() });
      }
      act(() => state.setReasoningEffort(undefined));
      await send('Return to model default');
      expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0]).not.toHaveProperty(
        'reasoning_effort'
      );
      emitAgent({ type: 'done', conversation_id: activeSession() });
    }
    act(() => state.setReasoningEffort('medium'));
    act(() => renderer?.unmount());
    renderer = undefined;
    await mount();
    expect(state.reasoningEffort).toBe('medium');
    expect(api.sendAgentMessage).toHaveBeenCalledTimes(6 * (levels.length + 1));
    act(() => state.setReasoningEffort(undefined));
    await send('Return to model default');
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0]).not.toHaveProperty(
      'reasoning_effort'
    );
  });

  it('saves next-turn effort during pending startup and streaming without interrupting the active run', async () => {
    const startup = deferred<string>();
    const unlisten = vi.fn();
    vi.mocked(api.sendAgentMessage).mockReturnValueOnce(startup.promise);
    vi.mocked(api.listenAgentEvents).mockImplementation(async (handler) => {
      agentEvent = handler;
      return unlisten;
    });
    await mount();
    act(() => state.setReasoningEffort('max'));
    await send('Complex task');
    const activeRequest = vi.mocked(api.sendAgentMessage).mock.lastCall![0];
    const originalRequest = { ...activeRequest };
    const conversation_id = activeSession();
    act(() => state.setReasoningEffort('low'));
    expect(state.reasoningEffort).toBe('low');
    expect(state.isProcessing).toBe(true);
    expect(useNavigationStore.getState().agentRunning).toBe(true);
    await act(async () => startup.resolve('ok'));
    emitAgent({ type: 'text_delta', text: 'Still working', conversation_id });
    emitAgent({ type: 'tool_call_start', id: 'tool', name: 'check', conversation_id });
    act(() => state.setReasoningEffort(undefined));
    emitAgent({ type: 'tool_call_args', id: 'tool', args: '{}', conversation_id });
    emitAgent({
      type: 'tool_result',
      id: 'tool',
      output: 'Checked',
      success: true,
      conversation_id,
    });
    act(() => state.setReasoningEffort('low'));
    emitAgent({ type: 'text_delta', text: 'Finished', conversation_id });
    expect(state.chatOutput).toContainEqual({ type: 'assistant', text: 'Still working' });
    expect(state.chatOutput).toContainEqual({ type: 'assistant', text: 'Finished' });
    expect(state.chatOutput).toContainEqual({
      type: 'tool_call',
      id: 'tool',
      name: 'check',
      args: '{}',
      status: 'done',
      output: 'Checked',
    });
    expect(state.isProcessing).toBe(true);
    expect(useNavigationStore.getState().agentRunning).toBe(true);
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
    expect(activeRequest).toEqual(originalRequest);
    expect(activeRequest.reasoning_effort).toBe('max');
    expect(api.abortAgent).not.toHaveBeenCalled();
    expect(api.parasiteAbort).not.toHaveBeenCalled();
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
    expect(api.listenAgentEvents).toHaveBeenCalledOnce();
    expect(unlisten).not.toHaveBeenCalled();
    emitAgent({ type: 'done', conversation_id });
    expect(state.isProcessing).toBe(false);
    await send('Quick follow up');
    expect(api.sendAgentMessage).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0].reasoning_effort).toBe('low');
  });

  it('still blocks effort changes while conversation deletion is pending', async () => {
    await mount();
    act(() => state.setReasoningEffort('max'));
    const reset = deferred<string>();
    vi.mocked(api.resetConversationContext).mockReturnValueOnce(reset.promise);
    await act(async () => {
      void state.deleteConversation(state.activeConversationId);
    });
    act(() => state.setReasoningEffort('low'));
    expect(state.reasoningEffort).toBe('max');
    await act(async () => reset.resolve('ok'));
    act(() => state.setReasoningEffort('low'));
    await send('Quick follow up');
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0].reasoning_effort).toBe('low');
  });

  it('drops invalid saved values and keeps native Claude Code settings separate', async () => {
    stored.set(REASONING_STORAGE_KEY, JSON.stringify({ [preferenceKey(deepseek)]: 'unsupported' }));
    await mount();
    expect(state.reasoningEffort).toBeUndefined();
    const original = stored.get(REASONING_STORAGE_KEY);
    act(() => state.setReasoningEffort('medium'));
    expect(state.reasoningEffort).toBe('medium');
    expect(stored.get(REASONING_STORAGE_KEY)).not.toBe(original);
    act(() => state.setReasoningEffort('high'));
    act(() => state.setParasiteAgent('claudecode'));
    expect(state.reasoningLevels).toEqual([]);
    expect(state.reasoningEffort).toBeUndefined();
    await send('Native turn');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(vi.mocked(api.parasiteSendMessage).mock.lastCall?.[0]).not.toHaveProperty(
      'reasoningEffort'
    );
    emitParasite({ type: 'done', conversation_id: activeSession() });
    act(() => state.setParasiteAgent(null));
    expect(state.reasoningEffort).toBe('high');
    act(() => state.setAgentModel(model.internalId));
    act(() => state.setReasoningEffort('high'));
    await send('Unknown model');
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0].reasoning_effort).toBe('high');
  });

  it('invalidates a preference when an existing entry changes model and allows reset to default', async () => {
    await mount();
    act(() => state.setReasoningEffort('max'));
    vi.mocked(api.getModels).mockResolvedValue([
      { ...deepseek, modelId: 'deepseek-v4-pro' },
      gpt,
      model,
    ]);
    await act(async () => {
      window.dispatchEvent(new Event('models-changed'));
    });
    expect(state.reasoningEffort).toBeUndefined();
    act(() => state.setReasoningEffort('high'));
    act(() => state.setReasoningEffort(undefined));
    await send('Default again');
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0]).not.toHaveProperty(
      'reasoning_effort'
    );
  });

  it('preserves the chosen level and shows request failures without silently downgrading or retrying', async () => {
    await mount();
    act(() => state.setReasoningEffort('max'));
    vi.mocked(api.sendAgentMessage).mockRejectedValueOnce('fixture provider rejected request');
    await send('Test failure');
    expect(state.isProcessing).toBe(false);
    expect(state.reasoningEffort).toBe('max');
    expect(state.chatOutput).toContainEqual({
      type: 'error',
      text: 'fixture provider rejected request',
    });
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
    await send('Explicit retry');
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall?.[0].reasoning_effort).toBe('max');
  });
});

describe('Mother Agent smart router availability', () => {
  const router: ModelConfig = {
    ...model,
    internalId: 'smart-router',
    name: 'Auto Router',
    baseUrl: 'http://127.0.0.1:53683/v1',
  };

  async function toggleRouter(enabled: boolean) {
    routerEnabled = enabled;
    await act(async () => {
      renderer!.update(
        <MotherAgentProvider>
          <Harness />
        </MotherAgentProvider>
      );
    });
  }

  beforeEach(() => {
    vi.mocked(api.getModels).mockResolvedValue([model, router]);
  });

  it('keeps the latest router settings when an earlier model-list response arrives last', async () => {
    const initial = {
      ...router,
      internalId: 'smart-router-7d55a193-322a-46b2-aedb-47dbddcdb877',
    };
    const updated = {
      ...initial,
      name: 'Updated router',
      modelId: 'updated-auto',
      baseUrl: 'http://127.0.0.1:60001/v1',
      anthropicUrl: 'http://127.0.0.1:60001',
      apiKey: 'updated-router-key',
    };
    stored.set('echobird_agent_model', initial.internalId);
    vi.mocked(api.getModels).mockResolvedValue([model, initial]);
    await mount();
    const previous = deferred<ModelConfig[]>();
    const latest = deferred<ModelConfig[]>();
    vi.mocked(api.getModels)
      .mockReturnValueOnce(previous.promise)
      .mockReturnValueOnce(latest.promise);
    act(() => {
      window.dispatchEvent(new Event('focus'));
    });
    act(() => {
      window.dispatchEvent(new Event('models-changed'));
    });
    await act(async () => latest.resolve([model, updated]));
    await act(async () => previous.resolve([model, initial]));
    expect(state.models).toEqual([model, updated]);
    expect(state.agentModel).toBe(updated.internalId);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    await send('Use the current router settings');
    expect(api.sendAgentMessage).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({
        model_id: updated.internalId,
        base_url: updated.baseUrl,
        api_key: updated.apiKey,
        model_name: updated.modelId,
      })
    );
  });

  it('does not restore a deleted router from an older model-list response', async () => {
    stored.set('echobird_agent_model', router.internalId);
    await mount();
    const previous = deferred<ModelConfig[]>();
    const latest = deferred<ModelConfig[]>();
    vi.mocked(api.getModels)
      .mockResolvedValue([model])
      .mockReturnValueOnce(previous.promise)
      .mockReturnValueOnce(latest.promise);
    act(() => {
      window.dispatchEvent(new Event('focus'));
    });
    act(() => {
      window.dispatchEvent(new Event('models-changed'));
    });
    await act(async () => latest.resolve([model]));
    await act(async () => previous.resolve([model, router]));
    expect(state.models).toEqual([model]);
    expect(state.agentModel).toBeNull();
    expect(stored.has('echobird_agent_model')).toBe(false);
    await send('Do not use the deleted router');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
  });

  it('ignores a late model-list response after unmount without clearing the saved selection', async () => {
    stored.set('echobird_agent_model', router.internalId);
    const pending = deferred<ModelConfig[]>();
    vi.mocked(api.getModels).mockReturnValueOnce(pending.promise);
    await mount();
    act(() => renderer!.unmount());
    renderer = undefined;
    await act(async () => pending.resolve([]));
    expect(stored.get('echobird_agent_model')).toBe(router.internalId);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
  });

  it('clears the selected router when a successful model-list refresh is empty', async () => {
    stored.set('echobird_agent_model', router.internalId);
    vi.mocked(api.getModels).mockResolvedValue([router]);
    await mount();
    expect(state.agentModel).toBe(router.internalId);
    vi.mocked(api.getModels).mockResolvedValue([]);
    await act(async () => {
      window.dispatchEvent(new Event('models-changed'));
    });
    expect(state.models).toEqual([]);
    expect(state.agentModel).toBeNull();
    expect(stored.has('echobird_agent_model')).toBe(false);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    await send('Do not use the empty model list');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
  });

  it('preserves cached routers and selection when a model-list refresh fails', async () => {
    stored.set('echobird_agent_model', router.internalId);
    await mount();
    const cached = state.models;
    const errorLog = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    try {
      vi.mocked(api.getModels).mockRejectedValueOnce(new Error('Fixture model-list read failure'));
      await act(async () => {
        window.dispatchEvent(new Event('models-changed'));
      });
      expect(state.models).toBe(cached);
      expect(state.agentModel).toBe(router.internalId);
      expect(stored.get('echobird_agent_model')).toBe(router.internalId);
      expect(api.sendAgentMessage).not.toHaveBeenCalled();
      expect(confirm).not.toHaveBeenCalled();
      expect(showToast).not.toHaveBeenCalled();
      await send('Use the cached router after a read failure');
      expect(api.sendAgentMessage).toHaveBeenCalledExactlyOnceWith(
        expect.objectContaining({ model_id: router.internalId, base_url: router.baseUrl })
      );
    } finally {
      errorLog.mockRestore();
    }
  });

  it('hides a disabled router on entry and clears its remembered selection without choosing another model', async () => {
    routerEnabled = false;
    stored.set('echobird_agent_model', router.internalId);
    await mount();
    expect(state.models).toEqual([model]);
    expect(state.agentModel).toBeNull();
    expect(stored.has('echobird_agent_model')).toBe(false);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
    await send('Do not use the disabled router');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(state.chatOutput).toContainEqual({
      type: 'error',
      text: '',
      i18nKey: 'error.noModelSelected',
    });
  });

  it('updates the cached list immediately, preserves other selections, and does not restore a disabled selection', async () => {
    await mount();
    expect(state.models).toEqual([model, router]);
    const reads = vi.mocked(api.getModels).mock.calls.length;
    await toggleRouter(false);
    expect(state.models).toEqual([model]);
    expect(state.agentModel).toBe(model.internalId);
    expect(api.getModels).toHaveBeenCalledTimes(reads);
    await toggleRouter(true);
    expect(state.models).toEqual([model, router]);
    act(() => state.setAgentModel(router.internalId));
    await toggleRouter(false);
    expect(state.agentModel).toBeNull();
    await toggleRouter(true);
    expect(state.models).toEqual([model, router]);
    expect(state.agentModel).toBeNull();
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteSendMessage).not.toHaveBeenCalled();
    expect(confirm).not.toHaveBeenCalled();
  });

  it('keeps a late model-list response filtered after the router is disabled', async () => {
    const pending = deferred<ModelConfig[]>();
    vi.mocked(api.getModels).mockReturnValueOnce(pending.promise);
    await mount();
    await toggleRouter(false);
    await act(async () => pending.resolve([model, router]));
    expect(state.models).toEqual([model]);
    expect(state.agentModel).toBe(model.internalId);
    act(() => useNavigationStore.setState({ activePage: 'apps' }));
    act(() => useNavigationStore.setState({ activePage: 'mother' }));
    expect(state.models).toEqual([model]);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
  });

  it('preserves the active conversation and request when disabling the router, and blocks the next router turn', async () => {
    stored.set('echobird_agent_model', router.internalId);
    await mount();
    await send('Existing router turn');
    const request = { ...vi.mocked(api.sendAgentMessage).mock.lastCall![0] };
    const conversation = state.activeConversationId;
    const transcript = state.chatOutput;
    await toggleRouter(false);
    expect(state.models).toEqual([model]);
    expect(state.agentModel).toBeNull();
    expect(state.isProcessing).toBe(true);
    expect(state.activeConversationId).toBe(conversation);
    expect(state.chatOutput).toEqual(transcript);
    expect(vi.mocked(api.sendAgentMessage).mock.lastCall![0]).toEqual(request);
    expect(api.abortAgent).not.toHaveBeenCalled();
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteAbort).not.toHaveBeenCalled();
    expect(api.resetConversationContext).not.toHaveBeenCalled();
    emitAgent({ type: 'done', conversation_id: activeSession() });
    await send('Choose an available model first');
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
  });
});

describe('Mother Agent saved conversation workflow', () => {
  it('deletes only the confirmed record and preserves the active conversation and draft', async () => {
    await mount();
    const deleted = state.activeConversationId;
    const deletedKey = activeSession();
    act(() => state.createConversation());
    const current = state.activeConversationId;
    await send('Keep current content');
    emitAgent({ type: 'done', conversation_id: activeSession() });
    act(() => state.setChatInput('Keep current draft'));
    const messages = state.chatOutput;
    await act(async () => {
      await state.deleteConversation(deleted);
    });
    expect(confirm).toHaveBeenCalledOnce();
    expect(api.resetConversationContext).toHaveBeenCalledExactlyOnceWith(
      deletedKey,
      true,
      deletedKey
    );
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
    expect(state.activeConversationId).toBe(current);
    expect(state.chatOutput).toEqual(messages);
    expect(state.chatInput).toBe('Keep current draft');
    expect(state.conversations.some((c) => c.id === deleted)).toBe(false);
    expect(showToast).not.toHaveBeenCalled();
  });

  it('does nothing on a cancelled delete confirmation and blocks repeat actions while it is open', async () => {
    await mount();
    const current = state.activeConversationId;
    const decision = deferred<boolean>();
    confirm.mockReturnValueOnce(decision.promise);
    const callbacks = state;
    let task!: Promise<void>;
    act(() => {
      task = callbacks.deleteConversation(current);
      void callbacks.deleteConversation(current);
      callbacks.createConversation();
      callbacks.selectServer('remote');
      callbacks.sendMessage('Blocked');
    });
    expect(confirm).toHaveBeenCalledOnce();
    expect(state.isProcessing).toBe(false);
    expect(state.isResetting).toBe(false);
    expect(state.activeConversationId).toBe(current);
    await act(async () => {
      decision.resolve(false);
      await task;
    });
    expect(state.isProcessing).toBe(false);
    expect(state.conversations).toHaveLength(1);
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.resetConversationContext).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
  });

  it('preserves saved records and shows an error when deletion cleanup fails', async () => {
    await mount();
    const current = state.activeConversationId;
    vi.mocked(api.resetConversationContext).mockRejectedValueOnce(
      new Error('Cannot delete saved context')
    );
    await act(async () => {
      await state.deleteConversation(current);
    });
    expect(state.activeConversationId).toBe(current);
    expect(state.conversations).toHaveLength(1);
    expect(api.parasiteReset).not.toHaveBeenCalled();
    expect(state.chatOutput).toEqual([]);
    expect(showToast).toHaveBeenCalledExactlyOnceWith(
      'error',
      'Error: Cannot delete saved context'
    );
    expect(state.isProcessing).toBe(false);
  });

  it('blocks deletion during generation and clears the draft only when deleting the active conversation', async () => {
    await mount();
    const current = state.activeConversationId;
    await send('Pending turn');
    await act(async () => {
      await state.deleteConversation(current);
    });
    expect(confirm).not.toHaveBeenCalled();
    emitAgent({ type: 'done', conversation_id: activeSession() });
    act(() => state.setChatInput('Old draft'));
    await act(async () => {
      await state.deleteConversation(current);
    });
    expect(state.activeConversationId).not.toBe(current);
    expect(state.chatInput).toBe('');
    expect(state.chatOutput).toEqual([]);
    expect(state.conversations).toHaveLength(1);
  });

  it('preserves the transcript after a composite delete failure and can continue the same conversation', async () => {
    await mount();
    const current = state.activeConversationId;
    const session = activeSession();
    await send('Keep original context');
    emitAgent({ type: 'text_delta', text: 'Original reply', conversation_id: session });
    emitAgent({ type: 'done', conversation_id: session });
    const transcript = state.chatOutput;
    vi.mocked(api.resetConversationContext).mockRejectedValueOnce(
      new Error('Cannot stage second context')
    );
    await act(async () => {
      await state.deleteConversation(current);
    });
    expect(state.activeConversationId).toBe(current);
    expect(state.chatOutput).toEqual(transcript);
    expect(state.conversations).toHaveLength(1);
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
    await send('Continue remembered context');
    expect(api.sendAgentMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({
        conversation_id: session,
        message: 'Continue remembered context',
      })
    );
  });

  it('leaves the shared legacy local CLI context out of a migrated remote conversation deletion', async () => {
    stored.set(
      'echobird:chat:agent_remote',
      JSON.stringify([{ role: 'user', content: 'Remote history' }])
    );
    await mount();
    act(() => state.selectServer('remote'));
    expect(state.activeConversationId).toBe('legacy-remote');
    await act(async () => {
      await state.deleteConversation('legacy-remote');
    });
    expect(api.resetConversationContext).toHaveBeenCalledExactlyOnceWith(
      'remote',
      false,
      undefined
    );
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
  });

  it('uses one composite reset for Connect and preserves history when its cleanup fails', async () => {
    stored.set('echobird_parasite_agent', 'claudecode');
    await mount();
    const current = state.activeConversationId;
    const session = activeSession();
    await send('Keep Connect context');
    emitParasite({ type: 'text_delta', text: 'Connect reply', conversation_id: session });
    emitParasite({ type: 'done', conversation_id: session });
    vi.mocked(api.resetConversationContext).mockRejectedValueOnce(
      new Error('Cannot stage second context')
    );
    await act(async () => {
      await state.deleteConversation(current);
    });
    expect(state.activeConversationId).toBe(current);
    expect(state.chatOutput.slice(0, 2)).toEqual([
      { type: 'user', text: 'Keep Connect context', chips: undefined },
      { type: 'assistant', text: 'Connect reply' },
    ]);
    expect(api.resetConversationContext).toHaveBeenCalledExactlyOnceWith(session, true, session);
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
    expect(state.isProcessing).toBe(false);
    await send('Continue Connect context');
    expect(api.parasiteSendMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({
        conversationId: session,
        message: 'Continue Connect context',
      })
    );
  });

  it('keeps mount, page navigation, new conversations, and selection free of execution or resets', async () => {
    await mount();
    const first = state.activeConversationId;
    act(() => useNavigationStore.getState().setActivePage('apps'));
    act(() => useNavigationStore.getState().setActivePage('mother'));
    act(() => state.createConversation());
    const second = state.activeConversationId;
    expect(second).not.toBe(first);
    act(() => state.selectConversation(first));
    expect(state.activeConversationId).toBe(first);
    act(() => state.selectServer('remote'));
    expect(state.selectedServerId).toBe('remote');
    act(() => state.selectServer('local'));
    expect(state.selectedServerId).toBe('local');
    for (const method of [
      api.sendAgentMessage,
      api.abortAgent,
      api.resetAgent,
      api.resetConversationContext,
      api.parasiteSendMessage,
      api.parasiteAbort,
      api.parasiteReset,
      api.saveSSHServer,
    ])
      expect(method).not.toHaveBeenCalled();
    expect(api.getModels).toHaveBeenCalled();
    expect(state.conversations).toHaveLength(2);
  });

  it('keeps an unsent draft when creating or selecting a conversation, then clears it on send', async () => {
    await mount();
    const first = state.activeConversationId;
    act(() => state.setChatInput('Unsent draft'));
    act(() => state.createConversation());
    const second = state.activeConversationId;
    expect(state.chatInput).toBe('Unsent draft');
    act(() => state.selectConversation(first));
    expect(state.chatInput).toBe('Unsent draft');
    act(() => state.selectConversation(second));
    expect(state.chatInput).toBe('Unsent draft');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(api.resetConversationContext).not.toHaveBeenCalled();

    const session = activeSession();
    await act(async () => state.handleChatSend());
    expect(state.chatInput).toBe('');
    expect(api.sendAgentMessage).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ conversation_id: session, message: 'Unsent draft' })
    );
  });

  it('sends independent session keys while preserving the selected server as the execution target', async () => {
    await mount();
    act(() => state.selectServer('remote'));
    const firstId = state.activeConversationId;
    const firstKey = activeSession();
    await send('First prompt');
    expect(api.sendAgentMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({ conversation_id: firstKey, server_ids: ['remote'] })
    );
    expect(firstKey).not.toBe('remote');
    emitAgent({ type: 'text_delta', text: 'First reply', conversation_id: firstKey });
    emitAgent({ type: 'done', conversation_id: firstKey });
    act(() => state.createConversation());
    const secondKey = activeSession();
    expect(secondKey).not.toBe(firstKey);
    expect(state.chatOutput).toEqual([]);
    await send('Second prompt');
    emitAgent({ type: 'text_delta', text: 'Second reply', conversation_id: secondKey });
    emitAgent({ type: 'done', conversation_id: secondKey });
    expect(api.sendAgentMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({ conversation_id: secondKey, server_ids: ['remote'] })
    );
    act(() => state.selectConversation(firstId));
    expect(state.chatOutput.map((m) => 'text' in m && m.text)).toEqual([
      'First prompt',
      'First reply',
    ]);
    expect(state.conversations.find((c) => c.sessionKey === secondKey)?.messages).toEqual([
      { type: 'user', text: 'Second prompt', chips: undefined },
      { type: 'assistant', text: 'Second reply' },
    ]);
    await act(async () => {
      await state.deleteConversation(firstId);
    });
    expect(api.resetConversationContext).toHaveBeenLastCalledWith(firstKey, true, firstKey);
    expect(state.conversations.some((c) => c.id === firstId)).toBe(false);
    expect(state.chatOutput).toEqual([
      { type: 'user', text: 'Second prompt', chips: undefined },
      { type: 'assistant', text: 'Second reply' },
    ]);
    expect(state.conversations.find((c) => c.sessionKey === secondKey)?.messages).toHaveLength(2);
  });

  it('publishes SSH changes only after durable storage and keeps failed rows', async () => {
    vi.mocked(api.loadSSHServers).mockResolvedValueOnce([]);
    await mount();
    const server = {
      id: 'durable',
      host: 'fixture.invalid',
      port: '22',
      username: 'fixture',
      password: 'fixture',
    };
    const revision = useNavigationStore.getState().sshServersVersion;
    vi.mocked(api.saveSSHServer).mockRejectedValueOnce('Cannot save SSH');
    await act(async () => {
      expect(await state.addSSHServer(server)).toBe(false);
    });
    expect(state.sshServers).toEqual([]);
    expect(useNavigationStore.getState().sshServersVersion).toBe(revision);
    expect(showToast).toHaveBeenCalledWith('error', 'Cannot save SSH');
    vi.mocked(api.saveSSHServer).mockResolvedValueOnce({ ...server, port: 22 });
    await act(async () => {
      expect(await state.addSSHServer(server)).toBe(true);
    });
    expect(state.sshServers).toHaveLength(1);
    act(() => state.selectServer(server.id));
    vi.mocked(api.removeSSHServerFromDisk).mockRejectedValueOnce('Cannot remove SSH');
    const savedRevision = useNavigationStore.getState().sshServersVersion;
    await act(async () => {
      await state.removeSSHServer(server.id);
    });
    expect(state.sshServers).toHaveLength(1);
    expect(state.selectedServerId).toBe(server.id);
    expect(useNavigationStore.getState().sshServersVersion).toBe(savedRevision);
    expect(showToast).toHaveBeenCalledWith('error', 'Cannot remove SSH');
    vi.mocked(api.removeSSHServerFromDisk).mockResolvedValueOnce(true);
    await act(async () => {
      await state.removeSSHServer(server.id);
    });
    expect(state.sshServers).toEqual([]);
    expect(state.selectedServerId).toBe('local');
  });

  it('blocks new, select, server changes, delete, and duplicate send even before the pending render', async () => {
    await mount();
    const first = state.activeConversationId;
    act(() => state.createConversation());
    const current = state.activeConversationId;
    const callbacks = state;
    await act(async () => {
      callbacks.sendMessage('Running');
      callbacks.createConversation();
      callbacks.selectConversation(first);
      callbacks.selectServer('remote');
      void callbacks.deleteConversation(current);
      callbacks.removeSSHServer('remote');
      callbacks.sendMessage('Duplicate');
    });
    expect(state.isProcessing).toBe(true);
    expect(state.activeConversationId).toBe(current);
    expect(state.selectedServerId).toBe('local');
    expect(state.conversations).toHaveLength(2);
    expect(state.sshServers).toHaveLength(1);
    expect(api.sendAgentMessage).toHaveBeenCalledTimes(1);
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.removeSSHServerFromDisk).not.toHaveBeenCalled();
    act(() => {
      state.createConversation();
      state.selectConversation(first);
      state.selectServer('remote');
    });
    expect(state.activeConversationId).toBe(current);
  });

  it('ignores old, unscoped, and wrong-engine events while accepting replies scoped to the active run', async () => {
    await mount();
    const firstKey = activeSession();
    await send('Old prompt');
    emitAgent({ type: 'done', conversation_id: firstKey });
    act(() => state.createConversation());
    const currentKey = activeSession();
    await send('Current prompt');
    emitAgent({ type: 'text_delta', text: 'Old reply', conversation_id: firstKey });
    emitAgent({ type: 'error', message: 'Old failure', conversation_id: firstKey });
    emitAgent({ type: 'done', conversation_id: firstKey });
    emitAgent({ type: 'text_delta', text: 'Unscoped reply' });
    act(() =>
      agentEvent({ type: 'text_delta', text: 'Missing request ID', conversation_id: currentKey })
    );
    emitParasite({ type: 'text_delta', text: 'Wrong engine', conversation_id: currentKey });
    expect(state.isProcessing).toBe(true);
    expect(state.chatOutput).toEqual([{ type: 'user', text: 'Current prompt', chips: undefined }]);
    emitAgent({ type: 'text_delta', text: 'Current reply', conversation_id: currentKey });
    expect(state.chatOutput[state.chatOutput.length - 1]).toEqual({
      type: 'assistant',
      text: 'Current reply',
    });
    emitAgent({ type: 'done', conversation_id: currentKey });
    expect(state.isProcessing).toBe(false);
  });

  it('keeps late events and invocation rejection from an aborted run out of a later conversation', async () => {
    const pending = deferred<string>();
    vi.mocked(api.sendAgentMessage).mockReturnValueOnce(pending.promise);
    await mount();
    const firstKey = activeSession();
    await send('Cancelled prompt');
    act(() => {
      state.abortAgent();
      state.abortAgent();
    });
    expect(api.abortAgent).toHaveBeenCalledExactlyOnceWith(firstKey);
    expect(state.chatOutput.filter((m) => m.type === 'cancelled')).toHaveLength(1);
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(state.isProcessing).toBe(false);
    act(() => state.createConversation());
    const currentKey = activeSession();
    await send('Current prompt');
    await act(async () => pending.reject(new Error('Late invocation failure')));
    emitAgent({ type: 'text_delta', text: 'Late cancelled reply', conversation_id: firstKey });
    emitAgent({ type: 'done', conversation_id: firstKey });
    expect(state.isProcessing).toBe(true);
    expect(state.chatOutput).toEqual([{ type: 'user', text: 'Current prompt', chips: undefined }]);
    emitAgent({ type: 'text_delta', text: 'Accepted reply', conversation_id: currentKey });
    expect(state.chatOutput[state.chatOutput.length - 1]).toEqual({
      type: 'assistant',
      text: 'Accepted reply',
    });
  });

  it('keeps no-model and explicit invocation failures in the conversation that sent them', async () => {
    await mount();
    const first = state.activeConversationId;
    act(() => state.setAgentModel(null));
    await send('Without model');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(state.chatOutput[state.chatOutput.length - 1]).toEqual({
      type: 'error',
      text: '',
      i18nKey: 'error.noModelSelected',
    });
    expect(state.isProcessing).toBe(false);
    act(() => state.createConversation());
    act(() => state.setAgentModel(model.internalId));
    vi.mocked(api.sendAgentMessage).mockRejectedValueOnce(new Error('Fixture provider failure'));
    await send('Failing prompt');
    expect(state.chatOutput[state.chatOutput.length - 1]).toEqual({
      type: 'error',
      text: 'Error: Fixture provider failure',
    });
    expect(state.isProcessing).toBe(false);
    act(() => state.selectConversation(first));
    expect(state.chatOutput).toHaveLength(2);
    expect(state.chatOutput[0]).toEqual({ type: 'user', text: 'Without model', chips: undefined });
    expect(api.sendAgentMessage).toHaveBeenCalledTimes(1);
  });

  it('preserves the transcript and blocks conversation actions until deletion cleanup succeeds', async () => {
    await mount();
    const first = state.activeConversationId;
    act(() => state.createConversation());
    const current = state.activeConversationId;
    const session = activeSession();
    await send('Saved prompt');
    emitAgent({ type: 'text_delta', text: 'Saved reply', conversation_id: session });
    emitAgent({ type: 'done', conversation_id: session });
    const pending = deferred<string>();
    vi.mocked(api.resetConversationContext).mockReturnValueOnce(pending.promise);
    const callbacks = state;
    await act(async () => {
      void callbacks.deleteConversation(current);
      callbacks.createConversation();
      callbacks.selectConversation(first);
      callbacks.selectServer('remote');
      callbacks.sendMessage('Blocked during deletion');
      void callbacks.deleteConversation(current);
    });
    expect(state.activeConversationId).toBe(current);
    expect(state.chatOutput).toHaveLength(2);
    expect(state.isProcessing).toBe(true);
    expect(api.resetConversationContext).toHaveBeenCalledExactlyOnceWith(session, true, session);
    expect(api.sendAgentMessage).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve('ok'));
    expect(state.chatOutput).toEqual([]);
    expect(state.isProcessing).toBe(false);
    expect(state.conversations.some((c) => c.id === current)).toBe(false);
    expect(state.activeConversationId).toBe(first);
  });

  it('keeps the saved transcript and draft visible when deletion cleanup fails', async () => {
    await mount();
    const current = state.activeConversationId;
    const session = activeSession();
    await send('Keep this prompt');
    emitAgent({ type: 'text_delta', text: 'Keep this reply', conversation_id: session });
    emitAgent({ type: 'done', conversation_id: session });
    act(() => state.setChatInput('Keep this draft'));
    vi.mocked(api.resetConversationContext).mockRejectedValueOnce(
      new Error('Cannot reset a running session')
    );
    await act(async () => {
      await state.deleteConversation(current);
    });
    expect(state.activeConversationId).toBe(current);
    expect(state.chatOutput).toEqual([
      { type: 'user', text: 'Keep this prompt', chips: undefined },
      { type: 'assistant', text: 'Keep this reply' },
    ]);
    expect(state.chatInput).toBe('Keep this draft');
    expect(showToast).toHaveBeenCalledExactlyOnceWith(
      'error',
      'Error: Cannot reset a running session'
    );
    expect(state.isProcessing).toBe(false);
  });

  it.each(['mother', 'connect'] as const)(
    'ignores late %s events from an aborted turn after restarting the same conversation',
    async (engine) => {
      if (engine === 'connect') stored.set('echobird_parasite_agent', 'claudecode');
      await mount();
      const conversationId = state.activeConversationId;
      const session = activeSession();
      await send('Old turn');
      const oldRequestId =
        engine === 'mother'
          ? vi.mocked(api.sendAgentMessage).mock.lastCall![0].request_id
          : vi.mocked(api.parasiteSendMessage).mock.lastCall![0].requestId;
      expect(oldRequestId).toEqual(expect.any(String));
      act(() => state.abortAgent());
      act(() => {
        vi.advanceTimersByTime(3000);
      });
      await send('New turn');
      expect(state.activeConversationId).toBe(conversationId);
      const newRequestId =
        engine === 'mother'
          ? vi.mocked(api.sendAgentMessage).mock.lastCall![0].request_id
          : vi.mocked(api.parasiteSendMessage).mock.lastCall![0].requestId;
      expect(newRequestId).not.toBe(oldRequestId);
      const emit = engine === 'mother' ? emitAgent : emitParasite;
      emit({
        type: 'text_delta',
        text: 'Old turn reply',
        conversation_id: session,
        request_id: oldRequestId,
      });
      emit({ type: 'done', conversation_id: session, request_id: oldRequestId });
      expect(state.isProcessing).toBe(true);
      expect(state.chatOutput.some((m) => 'text' in m && m.text === 'Old turn reply')).toBe(false);
      emit({ type: 'text_delta', text: 'New turn reply', conversation_id: session });
      expect(state.chatOutput[state.chatOutput.length - 1]).toEqual({
        type: 'assistant',
        text: 'New turn reply',
      });
      emit({ type: 'done', conversation_id: session });
      expect(state.isProcessing).toBe(false);
    }
  );

  it('passes the conversation scope to Connect send, abort, and deletion cleanup and isolates its events', async () => {
    stored.set('echobird_parasite_agent', 'claudecode');
    await mount();
    const firstKey = activeSession();
    await send('Connect prompt');
    expect(api.parasiteSendMessage).toHaveBeenLastCalledWith({
      agentId: 'claudecode',
      conversationId: firstKey,
      requestId: expect.any(String),
      message: 'Connect prompt',
    });
    emitParasite({ type: 'text_delta', text: 'Ignored unscoped' });
    emitAgent({ type: 'text_delta', text: 'Wrong engine', conversation_id: firstKey });
    emitParasite({ type: 'text_delta', text: 'Connect reply', conversation_id: firstKey });
    expect(state.chatOutput.map((m) => 'text' in m && m.text)).toEqual([
      'Connect prompt',
      'Connect reply',
    ]);
    act(() => state.abortAgent());
    expect(api.parasiteAbort).toHaveBeenCalledExactlyOnceWith('claudecode', firstKey);
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    await act(async () => {
      await state.deleteConversation(state.activeConversationId);
    });
    expect(api.resetConversationContext).toHaveBeenCalledExactlyOnceWith(firstKey, true, firstKey);
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
    const secondKey = activeSession();
    await send('Another Connect prompt');
    emitParasite({ type: 'done', conversation_id: firstKey });
    emitParasite({ type: 'text_delta', text: 'Old Connect reply', conversation_id: firstKey });
    expect(state.isProcessing).toBe(true);
    expect(state.chatOutput).toHaveLength(1);
    emitParasite({ type: 'text_delta', text: 'New Connect reply', conversation_id: secondKey });
    emitParasite({ type: 'done', conversation_id: secondKey });
    expect(state.isProcessing).toBe(false);
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
  });

  it('preserves the native optional Connect scope only for migrated legacy conversations', async () => {
    stored.set('echobird_parasite_agent', 'claudecode');
    stored.set(
      'echobird:chat:agent_local',
      JSON.stringify([{ role: 'user', content: 'Legacy prompt' }])
    );
    await mount();
    expect(state.activeConversationId).toBe('legacy-local');
    expect(activeSession()).toBe('local');
    await send('Continue native session');
    expect(api.parasiteSendMessage).toHaveBeenLastCalledWith({
      agentId: 'claudecode',
      conversationId: undefined,
      requestId: expect.any(String),
      message: 'Continue native session',
    });
    emitParasite({ type: 'text_delta', text: 'Native reply' });
    expect(state.chatOutput[state.chatOutput.length - 1]).toEqual({
      type: 'assistant',
      text: 'Native reply',
    });
    act(() => state.abortAgent());
    expect(api.parasiteAbort).toHaveBeenCalledExactlyOnceWith('claudecode', undefined);
    emitParasite({ type: 'done' });
    await act(async () => {
      await state.deleteConversation('legacy-local');
    });
    expect(api.resetConversationContext).toHaveBeenCalledExactlyOnceWith('local', true, undefined);
    expect(api.resetAgent).not.toHaveBeenCalled();
    expect(api.parasiteReset).not.toHaveBeenCalled();
    const scopedKey = activeSession();
    await send('New independent session');
    expect(api.parasiteSendMessage).toHaveBeenLastCalledWith({
      agentId: 'claudecode',
      conversationId: scopedKey,
      requestId: expect.any(String),
      message: 'New independent session',
    });
    emitParasite({ type: 'text_delta', text: 'Late native reply' });
    expect(state.chatOutput).toHaveLength(1);
    emitParasite({ type: 'text_delta', text: 'Scoped reply', conversation_id: scopedKey });
    expect(state.chatOutput[state.chatOutput.length - 1]).toEqual({
      type: 'assistant',
      text: 'Scoped reply',
    });
  });
});
