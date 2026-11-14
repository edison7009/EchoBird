import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import type { AgentEvent, AgentRequest, ModelConfig } from '../../api/types';
import { CONVERSATIONS_STORAGE_KEY, conversationBodyKey } from '../../hooks/useChatPersistence';
import { useNavigationStore } from '../../stores/navigationStore';
import { MotherAgentProvider } from './MotherAgentProvider';
import { useMotherAgent } from './context';
import type { MotherAgentCtx } from './types';

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
  parasiteSendMessage: vi.fn(),
  parasiteAbort: vi.fn(),
  parasiteReset: vi.fn(),
  applyModelToTool: vi.fn(),
  restoreToolToOfficial: vi.fn(),
  addModel: vi.fn(),
  updateModel: vi.fn(),
  deleteModel: vi.fn(),
  saveSettings: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'en' }),
}));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../FreeModels/provider', () => ({ useFreeModels: () => ({ routerEnabled: true }) }));

const confirm = vi.fn();
const showToast = vi.fn();
const unlisten = vi.fn();
const openai: ModelConfig = {
  internalId: 'openai-model',
  name: 'OpenAI fixture',
  modelId: 'gpt-5.2',
  baseUrl: 'https://openai.example.test/v1',
  apiKey: 'openai-fixture-key',
};
const anthropic: ModelConfig = {
  internalId: 'anthropic-model',
  name: 'Anthropic fixture',
  modelId: 'claude-sonnet-4-5',
  baseUrl: '',
  anthropicUrl: 'https://anthropic.example.test/v1',
  apiKey: 'anthropic-fixture-key',
};
const dual: ModelConfig = {
  ...openai,
  internalId: 'dual-model',
  anthropicUrl: 'https://dual.example.test/anthropic/v1',
};
let state: MotherAgentCtx;
let renderer: ReactTestRenderer | undefined;
let stored: Map<string, string>;
let onAgentEvent: (event: AgentEvent) => void;

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

async function select(model: ModelConfig) {
  await act(async () => state.setAgentModel(model.internalId));
}

async function send(text: string) {
  await act(async () => {
    state.sendMessage(text);
  });
  return vi.mocked(api.sendAgentMessage).mock.lastCall![0];
}

function emit(request: AgentRequest, event: AgentEvent) {
  act(() =>
    onAgentEvent({
      ...event,
      conversation_id: request.conversation_id,
      request_id: request.request_id,
    })
  );
}

function expectNoContextOrClientWrites() {
  expect(api.abortAgent).not.toHaveBeenCalled();
  expect(api.resetAgent).not.toHaveBeenCalled();
  expect(api.parasiteAbort).not.toHaveBeenCalled();
  expect(api.parasiteReset).not.toHaveBeenCalled();
  expect(api.parasiteSendMessage).not.toHaveBeenCalled();
  expect(api.applyModelToTool).not.toHaveBeenCalled();
  expect(api.restoreToolToOfficial).not.toHaveBeenCalled();
  expect(api.addModel).not.toHaveBeenCalled();
  expect(api.updateModel).not.toHaveBeenCalled();
  expect(api.deleteModel).not.toHaveBeenCalled();
  expect(api.saveSettings).not.toHaveBeenCalled();
  expect(api.saveSSHServer).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
  expect(unlisten).not.toHaveBeenCalled();
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  stored = new Map([
    ['echobird_agent_model', openai.internalId],
    [
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        activeConversationId: 'saved-conversation',
        conversations: [
          {
            id: 'saved-conversation',
            sessionKey: 'saved-context',
            serverId: 'local',
            title: 'Earlier task',
            createdAt: 100,
            updatedAt: 200,
            messages: [
              { role: 'user', content: 'Earlier user instruction' },
              { role: 'assistant', content: 'Earlier assistant result' },
              {
                role: 'tool',
                content: JSON.stringify({
                  id: 'saved-tool',
                  name: 'shell_exec',
                  args: '{}',
                  status: 'done',
                  output: 'Earlier tool evidence',
                }),
              },
            ],
          },
        ],
      }),
    ],
  ]);
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
  vi.stubGlobal('crypto', { randomUUID: () => `fixture-request-${++sequence}` });
  useNavigationStore.setState({
    activePage: 'mother',
    motherPrefill: undefined,
    agentRunning: false,
  });
  vi.mocked(api.getModels).mockResolvedValue([openai, anthropic, dual]);
  vi.mocked(api.loadSSHServers).mockResolvedValue([]);
  vi.mocked(api.parasiteListInstalled).mockResolvedValue([]);
  vi.mocked(api.listenAgentEvents).mockImplementation(async (handler) => {
    onAgentEvent = handler;
    return unlisten;
  });
  vi.mocked(api.listenParasiteEvents).mockResolvedValue(vi.fn());
  vi.mocked(api.sendAgentMessage).mockResolvedValue('ok');
});

afterEach(() => {
  if (renderer) act(() => renderer!.unmount());
  renderer = undefined;
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('Mother Agent protocol switching', () => {
  it('uses one saved conversation and context for OpenAI → Anthropic → OpenAI explicit sends', async () => {
    await mount();
    const initial = state.chatOutput;
    expect(initial).toContainEqual({ type: 'assistant', text: 'Earlier assistant result' });
    expect(state.activeConversationId).toBe('saved-conversation');
    const first = await send('Continue with OpenAI');
    expect(first).toMatchObject({
      provider: 'openai',
      model_id: openai.internalId,
      base_url: openai.baseUrl,
      conversation_id: 'saved-context',
    });
    expect(first.anthropic_url).toBeUndefined();
    emit(first, { type: 'text_delta', text: 'OpenAI answer' });
    emit(first, { type: 'done' });
    const previous = state.chatOutput;
    act(() => state.setChatInput('Keep my draft'));
    await select(anthropic);
    expect(state.chatOutput).toEqual(previous);
    expect(state.chatInput).toBe('Keep my draft');
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
    expectNoContextOrClientWrites();
    const second = await send('Continue with Anthropic');
    expect(second).toMatchObject({
      provider: 'anthropic',
      model_id: anthropic.internalId,
      base_url: '',
      anthropic_url: anthropic.anthropicUrl,
      conversation_id: first.conversation_id,
    });
    emit(second, { type: 'text_delta', text: 'Anthropic answer' });
    emit(second, { type: 'done' });
    await select(openai);
    expect(api.sendAgentMessage).toHaveBeenCalledTimes(2);
    const third = await send('Continue with OpenAI again');
    expect(third).toMatchObject({
      provider: 'openai',
      model_id: openai.internalId,
      conversation_id: first.conversation_id,
    });
    expect(third.anthropic_url).toBeUndefined();
    emit(third, { type: 'text_delta', text: 'OpenAI resumed' });
    emit(third, { type: 'done' });
    expect(state.activeConversationId).toBe('saved-conversation');
    expect(state.conversations).toHaveLength(1);
    expect(state.chatOutput).toEqual([
      ...initial,
      { type: 'user', text: 'Continue with OpenAI', chips: undefined },
      { type: 'assistant', text: 'OpenAI answer' },
      { type: 'user', text: 'Continue with Anthropic', chips: undefined },
      { type: 'assistant', text: 'Anthropic answer' },
      { type: 'user', text: 'Continue with OpenAI again', chips: undefined },
      { type: 'assistant', text: 'OpenAI resumed' },
    ]);
    act(() => {
      vi.advanceTimersByTime(800);
    });
    const disk = JSON.parse(stored.get(CONVERSATIONS_STORAGE_KEY)!);
    disk.conversations[0].messages = JSON.parse(
      stored.get(conversationBodyKey(disk.conversations[0].id))!
    );
    expect(disk.activeConversationId).toBe('saved-conversation');
    expect(disk.conversations[0].sessionKey).toBe('saved-context');
    expect(
      disk.conversations[0].messages.map((message: { content: string }) => message.content)
    ).toContain('Earlier user instruction');
    expect(
      disk.conversations[0].messages.map((message: { content: string }) => message.content)
    ).toContain('Anthropic answer');
    expect(api.listenAgentEvents).toHaveBeenCalledOnce();
    expectNoContextOrClientWrites();
  });

  it('changes idle model/protocol preference without invoking a model or resetting saved history', async () => {
    await mount();
    const original = state.chatOutput;
    await select(anthropic);
    await select(dual);
    await select(openai);
    expect(state.agentModel).toBe(openai.internalId);
    expect(state.chatOutput).toEqual(original);
    expect(state.activeConversationId).toBe('saved-conversation');
    expect(api.sendAgentMessage).not.toHaveBeenCalled();
    expect(api.listenAgentEvents).toHaveBeenCalledOnce();
    expectNoContextOrClientWrites();
    await select(dual);
    const request = await send('Explicit send with both endpoints');
    expect(request.provider).toBe('openai');
    expect(request.anthropic_url).toBeUndefined();
    expect(request.conversation_id).toBe('saved-context');
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
    expectNoContextOrClientWrites();
  });

  it('switches model during a pending run without canceling it and applies the new protocol next turn', async () => {
    let finishStartup!: (value: string) => void;
    vi.mocked(api.sendAgentMessage).mockReturnValueOnce(
      new Promise((resolve) => {
        finishStartup = resolve;
      })
    );
    await mount();
    const running = await send('Long OpenAI task');
    const snapshot = { ...running };
    await select(anthropic);
    expect(state.isProcessing).toBe(true);
    expect(useNavigationStore.getState().agentRunning).toBe(true);
    expect(running).toEqual(snapshot);
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
    await send('Blocked while previous run is active');
    expect(api.sendAgentMessage).toHaveBeenCalledOnce();
    await act(async () => finishStartup('ok'));
    emit(running, { type: 'text_delta', text: 'Still the original run' });
    emit(running, { type: 'tool_call_start', id: 'live-tool', name: 'shell_exec' });
    emit(running, { type: 'tool_call_args', id: 'live-tool', args: '{"command":"check"}' });
    emit(running, {
      type: 'tool_result',
      id: 'live-tool',
      output: 'Original run evidence',
      success: true,
    });
    expect(state.chatOutput).toContainEqual({ type: 'assistant', text: 'Still the original run' });
    expect(state.chatOutput).toContainEqual({
      type: 'tool_call',
      id: 'live-tool',
      name: 'shell_exec',
      args: '{"command":"check"}',
      status: 'done',
      output: 'Original run evidence',
    });
    expect(state.isProcessing).toBe(true);
    expect(running.provider).toBe('openai');
    expectNoContextOrClientWrites();
    emit(running, { type: 'done' });
    const next = await send('Follow up using Anthropic');
    expect(next).toMatchObject({
      provider: 'anthropic',
      model_id: anthropic.internalId,
      conversation_id: running.conversation_id,
    });
    expect(state.activeConversationId).toBe('saved-conversation');
    expect(state.chatOutput).toContainEqual({
      type: 'user',
      text: 'Earlier user instruction',
      chips: undefined,
    });
    expect(api.sendAgentMessage).toHaveBeenCalledTimes(2);
    expect(api.listenAgentEvents).toHaveBeenCalledOnce();
    expectNoContextOrClientWrites();
  });
});
