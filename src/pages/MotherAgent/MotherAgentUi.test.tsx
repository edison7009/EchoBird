import React from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { loadLocale, translate, type TKey } from '../../i18n';
import * as api from '../../api/tauri';
import { ConversationList } from './ConversationList';
import { MotherAgentMain } from './MotherAgentMain';
import { MotherAgentPanel } from './MotherAgentPanel';
import { QuickCommandChips } from './QuickCommandChips';
import { ChatBubble, ToolCallCard } from '../../components/chat';
import { RemoteModelSelector } from '../../components/RemoteModelSelector';
import { ViewModeTabs } from '../../components/ViewModeTabs';
import type { MotherAgentCtx } from './types';

let state: Partial<MotherAgentCtx>;
let locale = 'en';
let activePage = 'mother';
let renderer: ReactTestRenderer | undefined;
const showToast = vi.hoisted(() => vi.fn());

vi.mock('./context', () => ({ useMotherAgent: () => state }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ locale, t: (key: TKey) => translate(key, locale) }),
}));
vi.mock('../../hooks/useDocumentVisible', () => ({ useDocumentVisible: () => true }));
vi.mock('../../stores/navigationStore', () => ({
  useNavigationStore: (selector: (value: { activePage: string }) => unknown) =>
    selector({ activePage }),
}));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => vi.fn(async () => true) }));
vi.mock('../../components/RemoteModelSelector', () => ({ RemoteModelSelector: () => null }));
vi.mock('../../components/chat', () => ({
  ChatBubble: ({ content }: { content: string }) => <span>{content}</span>,
  ToolCallCard: () => null,
}));
vi.mock('../../api/tauri', () => ({
  getMotherHints: vi.fn(async () => '{"hints":[{"action":"showSpecs"}]}'),
  getLlmServerInfo: vi.fn(async () => ({ running: false })),
}));

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-12T09:00:00Z'));
  vi.clearAllMocks();
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal('requestAnimationFrame', vi.fn());
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      disconnect() {}
    }
  );
  locale = 'en';
  activePage = 'mother';
  vi.stubGlobal('document', Object.assign(new EventTarget(), { visibilityState: 'visible' }));
  state = {
    conversations: [
      {
        id: 'first',
        serverId: 'local',
        title: 'Fix my environment',
        createdAt: Date.now() - 86_400_000,
        updatedAt: Date.now() - 86_400_000,
        sessionKey: 'first',
        messages: [
          { type: 'user', text: 'Check it' },
          { type: 'assistant', text: 'Done' },
          { type: 'tool_call', id: 'tool', name: 'check', args: '{}', status: 'done' },
          { type: 'error', text: 'An earlier failure' },
        ],
      },
      {
        id: 'second',
        serverId: 'local',
        title: '',
        createdAt: Date.now(),
        updatedAt: Date.now(),
        sessionKey: 'second',
        messages: [],
      },
    ],
    activeConversationId: 'first',
    createConversation: vi.fn(),
    selectConversation: vi.fn(),
    deleteConversation: vi.fn(async () => {}),
    isProcessing: false,
    isResetting: false,
    setChatInput: vi.fn(),
    selectedServerId: 'local',
    sshServers: [],
    models: [],
    chatInput: '',
    chatOutput: [],
    chatEndRef: { current: null },
    chatInputRef: { current: null },
    parasiteAgent: 'claudecode',
    parasiteAvailable: [],
    maDiskTotal: 0,
    loadOlderChat: vi.fn(async () => []),
    reasoningLevels: [],
    setReasoningEffort: vi.fn(),
  };
});

it('focuses an existing prefill when the first-visit input mounts late', () => {
  const focus = vi.fn();
  state = { ...state, chatInput: 'Install the selected application' };
  act(() => {
    renderer = create(<MotherAgentMain />, {
      createNodeMock: (element) => (element.type === 'textarea' ? { focus } : null),
    });
  });
  expect(focus).toHaveBeenCalledOnce();
  expect(state.handleChatSend).toBeUndefined();
});

it.each([false, true])(
  'places the question caret before the selected text after mounting (busy=%s)',
  (busy) => {
    const focus = vi.fn();
    const setSelectionRange = vi.fn();
    const draft = '提问：\n划词内容：Selected text 😀';
    const pending = {
      current: { text: draft, cursor: 3 } as { text: string; cursor: number } | null,
    };
    state = {
      ...state,
      chatInput: draft,
      chatPrefillRef: pending,
      isProcessing: busy,
      handleChatSend: vi.fn(),
    };
    act(() => {
      renderer = create(<MotherAgentMain />, {
        createNodeMock: (element) =>
          element.type === 'textarea' ? { focus, setSelectionRange } : null,
      });
    });
    if (busy) {
      expect(setSelectionRange).not.toHaveBeenCalled();
      state = { ...state, isProcessing: false };
      act(() => renderer!.update(<MotherAgentMain />));
    }
    expect(setSelectionRange).toHaveBeenCalledExactlyOnceWith(3, 3);
    expect(pending.current).toBeNull();
    state = { ...state, chatInput: '提问：My question\n划词内容：Selected text 😀' };
    act(() => renderer!.update(<MotherAgentMain />));
    expect(setSelectionRange).toHaveBeenCalledTimes(1);
    expect(state.handleChatSend).not.toHaveBeenCalled();
  }
);

it('keeps automatic memory operations out of the visible conversation', () => {
  state = {
    ...state,
    chatOutput: [
      { type: 'user', text: 'Continue the task' },
      ...Array.from({ length: 35 }, (_, index) => ({
        type: 'tool_call' as const,
        id: `memory-${index}`,
        name: 'memory_update',
        args: '{}',
        output: 'internal note',
        status: 'done' as const,
      })),
      {
        type: 'tool_call',
        id: 'shell',
        name: 'shell_exec',
        args: '{}',
        output: 'verified',
        status: 'done',
      },
      { type: 'assistant', text: 'Task completed' },
    ],
  };
  act(() => {
    renderer = create(<MotherAgentMain />);
  });
  const cards = renderer!.root.findAllByType(ToolCallCard);
  expect(cards).toHaveLength(1);
  expect(cards[0].props.name).toBe('shell_exec');
  expect(renderer!.root.findAllByType(ChatBubble).map((bubble) => bubble.props.content)).toEqual([
    'Continue the task',
    'Task completed',
  ]);
  expect(state.chatOutput).toHaveLength(38);
  expect(JSON.stringify(renderer!.toJSON())).not.toContain('internal note');
});

describe('Mother Agent visible history pagination', () => {
  const hidden = (count: number) =>
    Array.from({ length: count }, (_, index) => ({
      type: 'tool_call' as const,
      id: `memory-${index}`,
      name: 'memory_search',
      args: '{}',
      status: 'done' as const,
    }));
  const contents = () =>
    renderer!.root
      .findAllByType(ChatBubble)
      .filter((bubble) => bubble.props.role !== 'skeleton')
      .map((bubble) => bubble.props.content);
  const mountScrollable = async () => {
    const container = { scrollTop: 0, scrollHeight: 1000, clientHeight: 300, children: [] };
    await act(async () => {
      renderer = create(<MotherAgentMain />, {
        createNodeMock: (element) => (element.props.onScroll ? container : null),
      });
    });
    container.scrollTop = 0;
    return renderer!.root.find((node) => typeof node.props.onScroll === 'function');
  };

  it('windows and expands visible history while preserving the raw message count', async () => {
    const messages = Array.from({ length: 40 }, (_, index) => ({
      type: 'user' as const,
      text: `visible-${index}`,
    }));
    state = { ...state, chatOutput: [...messages, ...hidden(35)], maDiskTotal: 75 };
    const scrollArea = await mountScrollable();
    expect(contents()).toEqual(messages.slice(-30).map((message) => message.text));
    act(() => void scrollArea.props.onScroll());
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(contents()).toEqual(messages.map((message) => message.text));
    expect(state.chatOutput).toHaveLength(75);
    expect(state.loadOlderChat).not.toHaveBeenCalled();
    expect(scrollArea.props.className).toBe('absolute inset-0 overflow-y-auto slim-scroll p-4');
  });

  it('loads through hidden-only older chunks to reach visible history using raw disk progress', async () => {
    const recent = [
      { type: 'user' as const, text: 'Recent request' },
      { type: 'assistant' as const, text: 'Recent answer' },
    ];
    const earlier = [
      { type: 'user' as const, text: 'Earlier request' },
      { type: 'assistant' as const, text: 'Earlier answer' },
    ];
    const chunks = [hidden(35), hidden(20), earlier];
    const load = vi.fn(async () => {
      const older = chunks.shift()!;
      state = { ...state, chatOutput: [...older, ...state.chatOutput!] };
      renderer!.update(<MotherAgentMain />);
      return older;
    });
    state = { ...state, chatOutput: recent, maDiskTotal: 59, loadOlderChat: load };
    const scrollArea = await mountScrollable();
    expect(load).not.toHaveBeenCalled();
    await act(async () => scrollArea.props.onScroll());
    expect(load).toHaveBeenCalledTimes(3);
    expect(contents()).toEqual([
      'Earlier request',
      'Earlier answer',
      'Recent request',
      'Recent answer',
    ]);
    expect(state.chatOutput).toHaveLength(59);
    await act(async () => scrollArea.props.onScroll());
    expect(load).toHaveBeenCalledTimes(3);
  });

  it('stops hidden-only paging at the raw disk length or an empty chunk', async () => {
    const chunks = [hidden(30), hidden(10)];
    const load = vi.fn(async () => {
      const older = chunks.shift()!;
      state = { ...state, chatOutput: [...older, ...state.chatOutput!] };
      renderer!.update(<MotherAgentMain />);
      return older;
    });
    state = {
      ...state,
      chatOutput: [{ type: 'user', text: 'Keep visible' }],
      maDiskTotal: 41,
      loadOlderChat: load,
    };
    const scrollArea = await mountScrollable();
    await act(async () => scrollArea.props.onScroll());
    expect(load).toHaveBeenCalledTimes(2);
    expect(contents()).toEqual(['Keep visible']);
    expect(renderer!.root.findAllByProps({ role: 'skeleton' })).toHaveLength(0);
    load.mockResolvedValueOnce([]);
    state = { ...state, maDiskTotal: 80 };
    await act(async () => renderer!.update(<MotherAgentMain />));
    await act(async () => scrollArea.props.onScroll());
    expect(load).toHaveBeenCalledTimes(3);
    expect(contents()).toEqual(['Keep visible']);
  });

  it('does not continue old hidden paging after changing conversations', async () => {
    let resolve!: (older: ReturnType<typeof hidden>) => void;
    const load = vi.fn(() => new Promise<ReturnType<typeof hidden>>((done) => (resolve = done)));
    state = {
      ...state,
      chatOutput: [{ type: 'user', text: 'Old request' }],
      maDiskTotal: 100,
      loadOlderChat: load,
    };
    const scrollArea = await mountScrollable();
    act(() => void scrollArea.props.onScroll());
    state = {
      ...state,
      activeConversationId: 'second',
      chatOutput: [{ type: 'user', text: 'New request' }],
    };
    await act(async () => renderer!.update(<MotherAgentMain />));
    await act(async () => resolve(hidden(35)));
    expect(load).toHaveBeenCalledOnce();
    expect(contents()).toEqual(['New request']);
    expect(renderer!.root.findAllByProps({ role: 'skeleton' })).toHaveLength(0);
  });
});

it('changes model selection without clear advice or a legacy clear-chat listener', () => {
  const transcript = [{ type: 'user' as const, text: 'Continue this task' }];
  state = {
    ...state,
    agentModel: 'openai-model',
    parasiteAgent: null,
    chatOutput: transcript,
    setAgentModel: vi.fn(),
    setParasiteAgent: vi.fn(),
  };
  act(() => {
    renderer = create(<MotherAgentMain />);
  });
  const selector = renderer!.root.findByType(RemoteModelSelector);
  expect(selector.props.showMenuIcons).toBe(true);
  act(() => selector.props.onSelect('anthropic-model'));
  expect(state.setAgentModel).toHaveBeenCalledExactlyOnceWith('anthropic-model');
  expect(state.setParasiteAgent).toHaveBeenCalledExactlyOnceWith(null);
  act(() => selector.props.onSelect('claudecode'));
  expect(state.setParasiteAgent).toHaveBeenLastCalledWith('claudecode');
  expect(state.setAgentModel).toHaveBeenCalledTimes(1);
  expect(showToast).not.toHaveBeenCalled();
  expect(state.chatOutput).toEqual(transcript);
  expect(
    vi.mocked(window.addEventListener).mock.calls.some(([type]) => type === 'clear-chat')
  ).toBe(false);
});

it('keeps the composer effort popup usable while running and closes it on reset, model changes, and page exit', () => {
  state = {
    ...state,
    parasiteAgent: null,
    reasoningLevels: ['low', 'high', 'max'],
    reasoningEffort: 'high',
  };
  act(() => {
    renderer = create(<MotherAgentMain />);
  });
  const open = () =>
    act(() =>
      renderer!.root.findByProps({ 'aria-label': 'Reasoning effort: High' }).props.onClick()
    );
  const slider = () => renderer!.root.findAllByProps({ type: 'range' });
  open();
  expect(slider()).toHaveLength(1);
  state = { ...state, isProcessing: true };
  act(() => renderer!.update(<MotherAgentMain />));
  expect(slider()).toHaveLength(1);
  expect(
    renderer!.root.findByProps({ 'aria-label': 'Reasoning effort: High' }).props.disabled
  ).toBe(false);
  act(() => slider()[0].props.onChange({ currentTarget: { value: '1' } }));
  expect(state.setReasoningEffort).toHaveBeenCalledExactlyOnceWith('low');
  state.setReasoningEffort = vi.fn();
  state = { ...state, isProcessing: false };
  act(() => renderer!.update(<MotherAgentMain />));
  expect(slider()).toHaveLength(1);
  state = { ...state, isResetting: true };
  act(() => renderer!.update(<MotherAgentMain />));
  expect(slider()).toHaveLength(0);
  state = { ...state, isResetting: false };
  act(() => renderer!.update(<MotherAgentMain />));
  expect(slider()).toHaveLength(0);
  open();
  state = { ...state, agentModel: 'another-model' };
  act(() => renderer!.update(<MotherAgentMain />));
  expect(slider()).toHaveLength(0);
  open();
  activePage = 'apps';
  act(() => renderer!.update(<MotherAgentMain />));
  expect(slider()).toHaveLength(0);
  activePage = 'mother';
  act(() => renderer!.update(<MotherAgentMain />));
  expect(slider()).toHaveLength(0);
  expect(state.setReasoningEffort).not.toHaveBeenCalled();
});

afterEach(() => {
  if (renderer) act(() => renderer?.unmount());
  renderer = undefined;
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('Mother Agent panel tabs', () => {
  it.each(['en', 'zh-Hans', 'zh-Hant', 'ja'])(
    'keeps only the three existing tabs with the shared active effect in %s',
    async (language) => {
      locale = language;
      await loadLocale(locale);
      state = { ...state, parasiteAgent: null };
      act(() => {
        renderer = create(<MotherAgentPanel />);
      });
      const tabs = renderer!.root.findByType(ViewModeTabs);
      expect(tabs.findByType('div').props.className).toContain(
        'view-mode-tabs flex border border-cyber-border rounded-button overflow-hidden'
      );
      expect(tabs.parent?.props.className).toBe(
        'h-10 px-2 flex items-center justify-between bg-transparent'
      );
      const buttons = tabs.findAllByType('button');
      expect(buttons.map((button) => button.children[0])).toEqual(
        (['mother.conversations', 'mother.servers', 'mother.sshGuide'] as const).map((key) =>
          translate(key, locale)
        )
      );
      for (const [index, button] of buttons.entries()) {
        expect(button.props.className).toContain('view-mode-tab');
        expect(button.props.className).toContain('text-sm');
        expect(button.props.className).not.toContain('cursor-');
        expect(button.props.title).toBeUndefined();
        act(() => button.props.onClick());
        expect(buttons.map((tab) => tab.props['aria-pressed'])).toEqual(
          buttons.map((_, tabIndex) => tabIndex === index)
        );
        expect(renderer!.root.findAllByType('textarea')).toHaveLength(0);
        expect(renderer!.root.findAllByType('select')).toHaveLength(0);
      }
      expect(state.createConversation).not.toHaveBeenCalled();
      expect(state.selectConversation).not.toHaveBeenCalled();
      expect(state.deleteConversation).not.toHaveBeenCalled();
      expect(showToast).not.toHaveBeenCalled();
    }
  );
});

describe('Mother Agent conversation list', () => {
  it('shows the current conversation, saved titles, message count, and relative update time', () => {
    act(() => {
      renderer = create(<ConversationList />);
    });
    const buttons = renderer!.root.findAllByType('button');
    const markup = JSON.stringify(renderer!.toJSON());
    expect(buttons[1].props['aria-current']).toBe('true');
    expect(markup).toContain('Fix my environment');
    expect(markup).toContain('2 messages');
    expect(markup).toContain('1 day ago');
    expect(markup).toContain('0 messages');
    expect(markup).toContain('Just now');
    expect(buttons[3].props['aria-label']).toBe('New conversation');
  });

  it('starts or selects only on explicit clicks and blocks both actions while processing', () => {
    act(() => {
      renderer = create(<ConversationList />);
    });
    expect(state.createConversation).not.toHaveBeenCalled();
    expect(state.selectConversation).not.toHaveBeenCalled();
    let buttons = renderer!.root.findAllByType('button');
    act(() => buttons[0].props.onClick());
    act(() => buttons[3].props.onClick());
    expect(state.createConversation).toHaveBeenCalledTimes(1);
    expect(state.selectConversation).toHaveBeenCalledWith('second');
    state = { ...state, isProcessing: true };
    act(() => renderer!.update(<ConversationList />));
    buttons = renderer!.root.findAllByType('button');
    expect(buttons.every((button) => button.props.disabled)).toBe(true);
    act(() => buttons.forEach((button) => button.props.onClick({ stopPropagation: vi.fn() })));
    expect(state.createConversation).toHaveBeenCalledTimes(1);
    expect(state.selectConversation).toHaveBeenCalledTimes(1);
  });

  it('localizes metadata and refreshes its time while the list remains open', async () => {
    await loadLocale('zh-Hans');
    locale = 'zh-Hans';
    act(() => {
      renderer = create(<ConversationList />);
    });
    expect(JSON.stringify(renderer!.toJSON())).toContain('2 条消息');
    expect(JSON.stringify(renderer!.toJSON())).toContain('1天前');
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(JSON.stringify(renderer!.toJSON())).toContain('1分钟前');
  });

  it('deletes through a separate button without selecting the record and disables deletion while processing', () => {
    act(() => {
      renderer = create(<ConversationList />);
    });
    const deleteLabel = `${translate('btn.delete', locale)} ${translate('mother.newConversation', locale)}`;
    const deletion = renderer!.root.findByProps({ 'aria-label': deleteLabel });
    expect(deletion.parent?.type).toBe('div');
    const event = { stopPropagation: vi.fn() };
    act(() => deletion.props.onClick(event));
    expect(event.stopPropagation).toHaveBeenCalled();
    expect(state.deleteConversation).toHaveBeenCalledExactlyOnceWith('second');
    expect(state.selectConversation).not.toHaveBeenCalled();
    state = { ...state, isProcessing: true };
    act(() => renderer!.update(<ConversationList />));
    const disabled = renderer!.root.findByProps({ 'aria-label': deleteLabel });
    expect(disabled.props.disabled).toBe(true);
    act(() => disabled.props.onClick(event));
    expect(state.deleteConversation).toHaveBeenCalledTimes(1);
  });
});

describe('Mother Agent welcome screen', () => {
  it('keeps quick commands as editable input without sending a message', async () => {
    state = { ...state, sendMessage: vi.fn(), handleChatSend: vi.fn() };
    await act(async () => {
      renderer = create(<QuickCommandChips />);
    });
    const button = renderer!.root.findAllByType('button')[0];
    act(() => button.props.onClick());
    expect(state.setChatInput).toHaveBeenCalledWith('Show local machine specs');
    expect(state.sendMessage).not.toHaveBeenCalled();
    expect(state.handleChatSend).not.toHaveBeenCalled();
    expect(button.props.className).not.toContain('cursor-');
  });

  it('shows the existing logo and commands for an empty conversation and keeps the input', async () => {
    await act(async () => {
      renderer = create(<MotherAgentMain />);
    });
    expect(renderer!.root.findByType('img').props.src).toBe('/brand/bird.png');
    expect(JSON.stringify(renderer!.toJSON())).toContain('EchoBird');
    expect(JSON.stringify(renderer!.toJSON())).toContain('Show local machine specs');
    expect(renderer!.root.findByType('textarea').props.value).toBe('');
    state = { ...state, chatOutput: [{ type: 'user', text: 'Hello' }] };
    await act(async () => renderer!.update(<MotherAgentMain />));
    expect(renderer!.root.findAllByType(QuickCommandChips)).toHaveLength(0);
    expect(JSON.stringify(renderer!.toJSON())).toContain('Hello');
    expect(renderer!.root.findAllByType('textarea')).toHaveLength(1);
  });

  it('does not let delayed pagination from an old conversation change the newly selected one', async () => {
    const messages = Array.from({ length: 60 }, (_, i) => ({
      type: 'user' as const,
      text: `message-${i}`,
    }));
    state = { ...state, chatOutput: messages };
    const scrollContainer = {
      scrollTop: 0,
      scrollHeight: 1000,
      clientHeight: 300,
      children: [],
    };
    await act(async () => {
      renderer = create(<MotherAgentMain />, {
        createNodeMock: (element) => (element.props.onScroll ? scrollContainer : null),
      });
    });
    const scrollArea = renderer!.root.find((node) => typeof node.props.onScroll === 'function');
    scrollContainer.scrollTop = 0;
    act(() => void scrollArea.props.onScroll());
    state = { ...state, activeConversationId: 'second' };
    await act(async () => renderer!.update(<MotherAgentMain />));
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(renderer!.root.findAllByType(ChatBubble).map((bubble) => bubble.props.content)).toEqual(
      messages.slice(30).map((message) => message.text)
    );
    expect(api.getMotherHints).not.toHaveBeenCalled();
  });
});

it('opens the context slider by keyboard, changes its value, and hides it for a locked conversation', () => {
  state = {
    ...state,
    parasiteAgent: null,
    contextSize: 32768,
    contextSizeEditable: true,
    setContextSize: vi.fn(),
  };
  act(() => {
    renderer = create(<MotherAgentMain />);
  });
  const ring = () => renderer!.root.findByProps({ role: 'progressbar' });
  expect(ring().props['aria-valuenow']).toBe(0);
  expect(renderer!.root.findAllByProps({ type: 'range' })).toHaveLength(0);
  act(() => {
    ring().props.onKeyDown({ key: 'Enter', preventDefault: vi.fn() });
  });
  const slider = renderer!.root.findByProps({ type: 'range' });
  act(() => slider.props.onChange({ currentTarget: { value: '4' } }));
  expect(state.setContextSize).toHaveBeenCalledExactlyOnceWith(131072);
  state = { ...state, contextSize: 131072, contextSizeEditable: false };
  act(() => {
    renderer!.update(<MotherAgentMain />);
  });
  expect(renderer!.root.findAllByProps({ type: 'range' })).toHaveLength(0);
  expect(ring().props['aria-haspopup']).toBeUndefined();
});
