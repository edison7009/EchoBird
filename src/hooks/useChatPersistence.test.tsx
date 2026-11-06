import React, { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  CONVERSATIONS_STORAGE_KEY,
  conversationBodyKey,
  useChatPersistence,
  type ChatConversation,
  type DiskMsg,
} from './useChatPersistence';

type Message = DiskMsg & { transient?: boolean };
type SavedState = {
  version: number;
  activeConversationId: string;
  conversations: ChatConversation<DiskMsg>[];
};

const toDisk = (message: Message): DiskMsg | null =>
  message.transient
    ? null
    : {
        role: message.role,
        content: message.content,
        ...(message.chips && { chips: message.chips }),
      };
const fromDisk = (message: DiskMsg): Message => ({ ...message });
let state: ReturnType<typeof useChatPersistence<Message>>;
let renderer: ReactTestRenderer | undefined;
let storage: Map<string, string>;
let setItem: ReturnType<typeof vi.fn>;
let nextId: number;

function Harness() {
  const result = useChatPersistence({ toDisk, fromDisk });
  useLayoutEffect(() => {
    state = result;
  });
  return null;
}

function mount() {
  act(() => {
    renderer = create(<Harness />);
  });
}

function unmount() {
  act(() => renderer?.unmount());
  renderer = undefined;
}

function saved(): SavedState {
  const index = JSON.parse(storage.get(CONVERSATIONS_STORAGE_KEY)!);
  if (index.version === 2)
    index.conversations = index.conversations.map((c: ChatConversation<DiskMsg>) => ({
      ...c,
      messages: c.messages ?? JSON.parse(storage.get(conversationBodyKey(c.id)) ?? '[]'),
    }));
  return index;
}

function conversation(id: string, serverId: string, updatedAt: number): ChatConversation<DiskMsg> {
  return {
    id,
    serverId,
    sessionKey: id,
    title: id,
    createdAt: updatedAt - 1000,
    updatedAt,
    messages: [{ role: 'user', content: id }],
    messageCount: 1,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-12T09:00:00Z'));
  storage = new Map();
  nextId = 0;
  setItem = vi.fn((key: string, value: string) => storage.set(key, value));
  vi.stubGlobal('localStorage', {
    get length() {
      return storage.size;
    },
    key: (index: number) => [...storage.keys()][index] ?? null,
    getItem: (key: string) => storage.get(key) ?? null,
    setItem,
    removeItem: (key: string) => storage.delete(key),
  });
  vi.stubGlobal('crypto', { randomUUID: () => `conversation-${++nextId}` });
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
});

afterEach(() => {
  unmount();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('Mother Agent conversation persistence', () => {
  it('recovers local and SSH legacy histories with their original backend session keys', () => {
    const local = [{ role: 'user', content: 'Local question\nMore detail' }];
    const remote = [{ role: 'assistant', content: 'SSH reply' }];
    storage.set('echobird:chat:agent_local', JSON.stringify(local));
    storage.set('echobird:chat:agent_ssh-production', JSON.stringify(remote));
    storage.set('echobird:chat:agent_broken', '{');
    mount();
    expect(state.activeConversation.serverId).toBe('local');
    expect(state.activeConversation.sessionKey).toBe('local');
    expect(state.messages).toEqual(local);
    expect(state.activeConversation.title).toBe('Local question');
    expect(setItem).not.toHaveBeenCalled();

    act(() => state.selectServer('ssh-production'));
    expect(state.activeConversation.sessionKey).toBe('ssh-production');
    expect(state.messages).toEqual(remote);
    expect(state.conversations).toHaveLength(1);
    expect(saved().conversations).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ serverId: 'local', sessionKey: 'local', messages: local }),
        expect.objectContaining({
          serverId: 'ssh-production',
          sessionKey: 'ssh-production',
          messages: remote,
        }),
      ])
    );
    unmount();
    mount();
    expect(state.activeConversation.serverId).toBe('ssh-production');
    expect(state.activeConversation.sessionKey).toBe('ssh-production');
    expect(state.messages).toEqual(remote);
  });

  it('flushes separate conversations before rapid switching and again on unmount', () => {
    mount();
    const firstId = state.activeConversationId;
    act(() => state.setMessages([{ role: 'user', content: 'First question' }]));
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(setItem).not.toHaveBeenCalled();
    act(() => state.createConversation('local'));
    const secondId = state.activeConversationId;
    expect(secondId).not.toBe(firstId);
    expect(state.activeConversation.sessionKey).toBe(secondId);
    expect(state.messages).toEqual([]);
    expect(saved().conversations.find((c) => c.id === firstId)?.messages).toEqual([
      { role: 'user', content: 'First question' },
    ]);

    act(() => state.setMessages([{ role: 'user', content: 'Second question' }]));
    act(() => state.selectConversation(firstId));
    expect(state.messages).toEqual([{ role: 'user', content: 'First question' }]);
    expect(saved().conversations.find((c) => c.id === secondId)?.messages).toEqual([
      { role: 'user', content: 'Second question' },
    ]);
    act(() =>
      state.setMessages((previous) => [...previous, { role: 'assistant', content: 'First reply' }])
    );
    unmount();
    mount();
    expect(state.activeConversationId).toBe(firstId);
    expect(state.messages).toEqual([
      { role: 'user', content: 'First question' },
      { role: 'assistant', content: 'First reply' },
    ]);
    act(() => state.selectConversation(secondId));
    expect(state.messages).toEqual([{ role: 'user', content: 'Second question' }]);
  });

  it('keeps existing update times and history order when a saved conversation is selected', () => {
    const older = conversation('older', 'local', Date.now() - 60_000);
    const newer = conversation('newer', 'local', Date.now());
    storage.set(
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({ version: 1, activeConversationId: newer.id, conversations: [older, newer] })
    );
    mount();
    act(() => state.selectConversation(older.id));
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(state.activeConversationId).toBe(older.id);
    expect(state.conversations.map((c) => c.id)).toEqual([newer.id, older.id]);
    expect(saved().conversations).toEqual([older, newer]);
    expect(saved().activeConversationId).toBe(older.id);
  });

  it('keeps server histories isolated and restores the latest conversation for each server', () => {
    mount();
    const localId = state.activeConversationId;
    act(() => state.setMessages([{ role: 'user', content: 'Local work' }]));
    act(() => state.selectServer('ssh-production'));
    const remoteId = state.activeConversationId;
    expect(state.messages).toEqual([]);
    expect(state.activeConversation.serverId).toBe('ssh-production');
    act(() => state.setMessages([{ role: 'user', content: 'Remote work' }]));
    act(() => state.createConversation('ssh-production'));
    const newerRemoteId = state.activeConversationId;
    act(() => {
      vi.advanceTimersByTime(1);
    });
    act(() => state.setMessages([{ role: 'user', content: 'New remote work' }]));
    expect(state.conversations.map((c) => c.id)).toEqual([newerRemoteId, remoteId]);

    act(() => state.selectServer('local'));
    expect(state.activeConversationId).toBe(localId);
    expect(state.messages).toEqual([{ role: 'user', content: 'Local work' }]);
    expect(state.conversations.map((c) => c.id)).toEqual([localId]);
    act(() => state.selectServer('ssh-production'));
    expect(state.activeConversationId).toBe(newerRemoteId);
    expect(state.messages).toEqual([{ role: 'user', content: 'New remote work' }]);
  });

  it('does not write storage during passive empty mounts, timer ticks, or unmount', () => {
    mount();
    expect(state.messages).toEqual([]);
    expect(state.activeConversation.serverId).toBe('local');
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    unmount();
    expect(setItem).not.toHaveBeenCalled();
    expect(storage.size).toBe(0);
  });

  it('falls back from a malformed index and discards invalid legacy messages', () => {
    storage.set(CONVERSATIONS_STORAGE_KEY, '{');
    storage.set(
      'echobird:chat:agent_local',
      JSON.stringify([
        null,
        { role: 'user' },
        { role: 1, content: 'Bad role' },
        { role: 'user', content: 'Recovered' },
        { role: 'assistant', content: 'Saved reply' },
      ])
    );
    mount();
    expect(state.messages).toEqual([
      { role: 'user', content: 'Recovered' },
      { role: 'assistant', content: 'Saved reply' },
    ]);
    expect(state.activeConversation.sessionKey).toBe('local');
    expect(setItem).not.toHaveBeenCalled();
  });

  it('keeps valid indexed conversations and messages when other saved entries are damaged', () => {
    const valid = conversation('valid', 'local', Date.now());
    storage.set(
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        activeConversationId: 'missing',
        conversations: [
          null,
          { id: 'broken' },
          { ...valid, messages: [null, { role: 'assistant', content: 12 }, ...valid.messages] },
        ],
      })
    );
    mount();
    expect(state.activeConversationId).toBe('valid');
    expect(state.messages).toEqual(valid.messages);
    expect(state.conversations).toHaveLength(1);
    expect(setItem).not.toHaveBeenCalled();
  });

  it('retains cached messages after a failed save and retries on the next content change', () => {
    setItem.mockImplementationOnce(() => {
      throw new Error('Storage full');
    });
    mount();
    act(() => state.setMessages([{ role: 'user', content: 'Keep this question' }]));
    act(() => {
      vi.advanceTimersByTime(800);
    });
    expect(setItem).toHaveBeenCalledTimes(1);
    expect(storage.has(CONVERSATIONS_STORAGE_KEY)).toBe(false);
    expect(state.messages).toEqual([{ role: 'user', content: 'Keep this question' }]);
    act(() =>
      state.setMessages((previous) => [
        ...previous,
        { role: 'assistant', content: 'Keep this reply' },
      ])
    );
    act(() => {
      vi.advanceTimersByTime(800);
    });
    expect(setItem).toHaveBeenCalledTimes(3);
    expect(saved().conversations[0].messages).toEqual(state.messages);
    unmount();
    mount();
    expect(state.messages).toEqual([
      { role: 'user', content: 'Keep this question' },
      { role: 'assistant', content: 'Keep this reply' },
    ]);
  });

  it('deletes the active legacy conversation without restoring it or changing other records', async () => {
    storage.set(
      'echobird:chat:agent_local',
      JSON.stringify([{ role: 'user', content: 'Legacy question' }])
    );
    mount();
    const firstId = state.activeConversationId;
    act(() => state.createConversation('local'));
    const secondId = state.activeConversationId;
    act(() => state.setMessages([{ role: 'user', content: 'Keep the other conversation' }]));
    act(() => state.selectConversation(firstId));
    await act(async () => state.deleteConversation(firstId));
    expect(state.conversations.some((conversation) => conversation.id === firstId)).toBe(false);
    expect(state.activeConversationId).toBe(secondId);
    unmount();
    mount();
    expect(state.activeConversationId).toBe(secondId);
    expect(state.conversations.some((conversation) => conversation.id === firstId)).toBe(false);
    expect(state.messages).toEqual([{ role: 'user', content: 'Keep the other conversation' }]);
    act(() => state.createConversation('local'));
    expect(state.messages).toEqual([]);
  });

  it('round-trips mapper-supported tool and error records and attachment chips without transient activity', () => {
    const messages: Message[] = [
      { role: 'user', content: 'Inspect this', chips: [{ type: 'file', name: 'report.md' }] },
      {
        role: 'tool_call',
        content: JSON.stringify({ name: 'inspect', status: 'done', output: 'Found it' }),
      },
      { role: 'error', content: 'An earlier request failed' },
      { role: 'assistant', content: 'Finished' },
      { role: 'working', content: 'Thinking', transient: true },
    ];
    mount();
    act(() => state.setMessages(messages));
    unmount();
    mount();
    expect(state.messages).toEqual(messages.slice(0, -1));
    expect(state.activeConversation.title).toBe('Inspect this');
  });

  it('deletes another conversation without changing the active record, its content, or recency', async () => {
    const older = conversation('older', 'local', Date.now() - 60_000);
    const active = conversation('active', 'local', Date.now());
    storage.set(
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        activeConversationId: active.id,
        conversations: [older, active],
      })
    );
    mount();
    await act(async () => {
      await state.deleteConversation(older.id);
    });
    expect(state.activeConversationId).toBe(active.id);
    expect(state.activeConversation).toEqual(active);
    expect(saved().conversations).toEqual([active]);
    unmount();
    mount();
    expect(state.conversations).toEqual([active]);
  });

  it('selects the newest remaining record on the same server and replaces the last deleted record with an empty one', async () => {
    const older = conversation('older', 'local', Date.now() - 60_000);
    const newer = conversation('newer', 'local', Date.now());
    const remote = conversation('remote', 'ssh-production', Date.now());
    storage.set(
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        activeConversationId: newer.id,
        conversations: [older, newer, remote],
      })
    );
    mount();
    await act(async () => {
      await state.deleteConversation(newer.id);
    });
    expect(state.activeConversationId).toBe(older.id);
    expect(state.activeConversation).toEqual(older);
    await act(async () => {
      await state.deleteConversation(older.id);
    });
    expect(state.activeConversation.serverId).toBe('local');
    expect(state.messages).toEqual([]);
    expect(state.activeConversationId).not.toBe(older.id);
    expect(saved().conversations).toContainEqual(remote);
    unmount();
    mount();
    expect(state.conversations).toHaveLength(1);
    expect(state.messages).toEqual([]);
  });

  it('keeps cached and persisted records when saving a deletion fails', async () => {
    const active = conversation('active', 'local', Date.now());
    storage.set(
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({ version: 1, activeConversationId: active.id, conversations: [active] })
    );
    mount();
    setItem.mockImplementationOnce(() => {
      throw new Error('Storage unavailable');
    });
    const cleanup = vi.fn(async () => {});
    await act(async () => {
      await expect(state.deleteConversation(active.id, cleanup)).rejects.toThrow(
        'Storage unavailable'
      );
    });
    expect(cleanup).not.toHaveBeenCalled();
    expect(state.activeConversation).toEqual(active);
    expect(saved().conversations).toEqual([active]);
  });

  it('restores the persisted row when context cleanup fails after the deletion was saved', async () => {
    const active = conversation('active', 'local', Date.now());
    storage.set(
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({ version: 1, activeConversationId: active.id, conversations: [active] })
    );
    mount();
    const cleanup = vi.fn(async () => {
      expect(saved().conversations.some((c) => c.id === active.id)).toBe(false);
      throw new Error('Context file locked');
    });
    await act(async () => {
      await expect(state.deleteConversation(active.id, cleanup)).rejects.toThrow(
        'Context file locked'
      );
    });
    expect(cleanup).toHaveBeenCalledOnce();
    expect(state.activeConversation).toEqual(active);
    expect(saved().conversations).toEqual([active]);
    unmount();
    mount();
    expect(state.activeConversation).toEqual(active);
  });
});

describe('conversation body loading and migration cost', () => {
  it('maps only the active history and rewrites only its body after migration', () => {
    const histories = Array.from({ length: 24 }, (_, index) => ({
      ...conversation(`history-${index}`, 'local', Date.now() - index),
      messages: Array.from({ length: 80 }, () => ({
        role: 'assistant',
        content: 'x'.repeat(2048),
      })),
    }));
    storage.set(
      CONVERSATIONS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        activeConversationId: histories[0].id,
        conversations: histories,
      })
    );
    const restore = vi.fn(fromDisk);
    function Measured() {
      const value = useChatPersistence({ toDisk, fromDisk: restore });
      useLayoutEffect(() => {
        state = value;
      });
      return (
        <>
          {value.conversations.map((conversation) => (
            <span key={conversation.id}>
              {conversation.messageCount ?? conversation.messages.length}
            </span>
          ))}
        </>
      );
    }
    act(() => {
      renderer = create(<Measured />);
    });
    expect(restore).toHaveBeenCalledTimes(80);
    act(() =>
      state.setMessages((messages) => [...messages, { role: 'user', content: 'new answer' }])
    );
    act(() => {
      vi.advanceTimersByTime(800);
    });
    expect(JSON.parse(storage.get(CONVERSATIONS_STORAGE_KEY)!).version).toBe(2);
    expect(saved().conversations).toHaveLength(24);
    setItem.mockClear();
    act(() =>
      state.setMessages((messages) => [...messages, { role: 'assistant', content: 'reply' }])
    );
    act(() => {
      vi.advanceTimersByTime(800);
    });
    expect(setItem.mock.calls.map(([key]) => key)).toEqual([
      conversationBodyKey(histories[0].id),
      CONVERSATIONS_STORAGE_KEY,
    ]);
    expect(setItem.mock.calls.reduce((size, [, value]) => size + value.length, 0)).toBeLessThan(
      180_000
    );
    unmount();
    restore.mockClear();
    act(() => {
      renderer = create(<Measured />);
    });
    expect(restore).toHaveBeenCalledTimes(82);
    act(() => state.selectConversation(histories[1].id));
    expect(restore).toHaveBeenCalledTimes(162);
    expect(state.messages).toEqual(histories[1].messages);
  });

  it('recovers every history when a migration body fails after partial progress', () => {
    const first = conversation('first', 'local', Date.now());
    const second = conversation('second', 'local', Date.now());
    const oldIndex = JSON.stringify({
      version: 1,
      activeConversationId: first.id,
      conversations: [first, second],
    });
    storage.set(CONVERSATIONS_STORAGE_KEY, oldIndex);
    setItem.mockImplementation((key, value) => {
      if (key === conversationBodyKey('second')) throw new Error('Storage full');
      storage.set(key, value);
    });
    mount();
    act(() =>
      state.setMessages((messages) => [...messages, { role: 'assistant', content: 'in memory' }])
    );
    act(() => {
      vi.advanceTimersByTime(800);
    });
    expect(saved().conversations.find((c) => c.id === 'first')?.messages).toEqual([
      ...first.messages,
      { role: 'assistant', content: 'in memory' },
    ]);
    const partial = JSON.parse(storage.get(CONVERSATIONS_STORAGE_KEY)!);
    expect(
      partial.conversations.find((c: ChatConversation<DiskMsg>) => c.id === 'second').messages
    ).toEqual(second.messages);
    expect(state.messages[state.messages.length - 1]?.content).toBe('in memory');
    setItem.mockImplementation((key, value) => storage.set(key, value));
    unmount();
    mount();
    expect(state.messages[state.messages.length - 1]?.content).toBe('in memory');
    act(() => state.selectConversation('second'));
    expect(state.messages).toEqual(second.messages);
  });
});

it('migrates large histories below the storage limit without duplicating the full archive', () => {
  const histories = Array.from({ length: 24 }, (_, index) => ({
    ...conversation(`large-${index}`, 'local', Date.now()),
    messages: Array.from({ length: 80 }, () => ({ role: 'assistant', content: 'x'.repeat(2048) })),
  }));
  storage.set(
    CONVERSATIONS_STORAGE_KEY,
    JSON.stringify({ version: 1, activeConversationId: histories[0].id, conversations: histories })
  );
  let peak = 0;
  setItem.mockImplementation((key, value) => {
    const total = [...storage].reduce(
      (size, [storedKey, storedValue]) => size + (storedKey === key ? 0 : storedValue.length),
      value.length
    );
    peak = Math.max(peak, total);
    if (total > 5_000_000) throw new Error('Quota exceeded');
    storage.set(key, value);
  });
  mount();
  act(() =>
    state.setMessages((messages) => [...messages, { role: 'user', content: 'new answer' }])
  );
  act(() => {
    vi.advanceTimersByTime(800);
  });
  expect(peak).toBeLessThan(5_000_000);
  expect(
    JSON.parse(storage.get(CONVERSATIONS_STORAGE_KEY)!).conversations.every(
      (c: ChatConversation<DiskMsg>) => c.messages === undefined
    )
  ).toBe(true);
  expect(saved().conversations[23].messages).toEqual(histories[23].messages);
  unmount();
  mount();
  act(() => state.selectConversation(histories[23].id));
  expect(state.messages).toEqual(histories[23].messages);
});

it('does not lose a deletion target if the preceding migration cannot complete', async () => {
  const first = conversation('first', 'local', Date.now());
  const second = conversation('second', 'local', Date.now());
  storage.set(
    CONVERSATIONS_STORAGE_KEY,
    JSON.stringify({ version: 1, activeConversationId: first.id, conversations: [first, second] })
  );
  mount();
  setItem.mockImplementation((key, value) => {
    if (key === conversationBodyKey(second.id)) throw new Error('No space');
    storage.set(key, value);
  });
  const cleanup = vi.fn(async () => {});
  await act(async () => {
    await expect(state.deleteConversation(first.id, cleanup)).rejects.toThrow('No space');
  });
  expect(cleanup).not.toHaveBeenCalled();
  expect(saved().conversations.map((c) => c.id)).toEqual([first.id, second.id]);
  expect(state.activeConversationId).toBe(first.id);
  unmount();
  mount();
  expect(state.messages).toEqual(first.messages);
});

it('preserves unsaved messages when saving and subsequent deletion cleanup both fail', async () => {
  mount();
  const id = state.activeConversationId;
  const original = [{ role: 'user', content: 'Saved question' }];
  act(() => state.setMessages(original));
  act(() => {
    vi.advanceTimersByTime(800);
  });
  let failures = 0;
  setItem.mockImplementation((key, value) => {
    if (key === conversationBodyKey(id) && failures++ < 2)
      throw new Error('Temporary storage failure');
    storage.set(key, value);
  });
  const latest = [...original, { role: 'assistant', content: 'Keep the unsaved reply' }];
  act(() => state.setMessages(latest));
  act(() => {
    vi.advanceTimersByTime(800);
  });
  const cleanup = vi.fn(async () => {
    throw new Error('Context file locked');
  });
  await act(async () => {
    await expect(state.deleteConversation(id, cleanup)).rejects.toThrow('Context file locked');
  });
  expect(cleanup).toHaveBeenCalledOnce();
  expect(state.messages).toEqual(latest);
  expect(saved().conversations.find((conversation) => conversation.id === id)?.messages).toEqual(
    latest
  );
  unmount();
  mount();
  expect(state.messages).toEqual(latest);
});

it('persists the selected context window separately for each conversation and preserves legacy metadata', () => {
  mount();
  expect(state.activeConversation.contextSize).toBe(32768);
  act(() => state.updateActiveConversation({ contextSize: 131072 }));
  const first = state.activeConversation.id;
  act(() => state.createConversation('local'));
  expect(state.activeConversation.contextSize).toBe(32768);
  act(() => state.selectConversation(first));
  expect(state.activeConversation.contextSize).toBe(131072);
  unmount();
  mount();
  expect(state.activeConversation.contextSize).toBe(131072);
  unmount();
  storage.clear();
  const legacy = conversation('legacy-window', 'local', Date.now());
  storage.set(
    CONVERSATIONS_STORAGE_KEY,
    JSON.stringify({ version: 1, activeConversationId: legacy.id, conversations: [legacy] })
  );
  mount();
  expect(state.activeConversation.contextSize).toBeUndefined();
});
