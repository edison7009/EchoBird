// Local conversation history. Model context is persisted separately by the backend.
import { useState, useRef, useEffect, useCallback } from 'react';
import type { BubbleChip } from '../components/chat/ChatBubble';

export interface DiskMsg {
  role: string;
  content: string;
  chips?: BubbleChip[];
}

export interface ChatConversation<T> {
  id: string;
  serverId: string;
  sessionKey: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: T[];
  messageCount?: number;
  /** Token context window chosen before the first message; absent on legacy history. */
  contextSize?: number;
}

interface ConversationState<T> {
  activeConversationId: string;
  conversations: ChatConversation<T>[];
}

export const CONVERSATIONS_STORAGE_KEY = 'echobird:agent:conversations';
export const conversationBodyKey = (id: string) => `echobird:agent:conversation:${id}`;
const LEGACY_PREFIX = 'echobird:chat:agent_';

function newConversation<T>(serverId: string): ChatConversation<T> {
  const id = crypto.randomUUID();
  return {
    id,
    serverId,
    sessionKey: id,
    title: '',
    createdAt: Date.now(),
    updatedAt: Date.now(),
    messages: [],
    messageCount: 0,
    contextSize: 32768,
  };
}

function readMessages(value: unknown): DiskMsg[] {
  return Array.isArray(value)
    ? value.filter(
        (m): m is DiskMsg => typeof m?.role === 'string' && typeof m?.content === 'string'
      )
    : [];
}

const countDiskMessages = (messages: DiskMsg[]) =>
  messages.filter((message) => message.role !== 'system' && message.role !== 'tool').length;

function loadConversations<T>(
  fromDisk: (m: DiskMsg) => T,
  pending: Map<string, DiskMsg[]>,
  stored: Set<string>
): ConversationState<T> {
  const readBody = (id: string) => {
    if (pending.has(id)) return pending.get(id)!;
    try {
      return readMessages(JSON.parse(localStorage.getItem(conversationBodyKey(id)) ?? '[]'));
    } catch {
      return [];
    }
  };
  const lazy = (conversation: ChatConversation<T>, read: () => DiskMsg[]) => {
    let messages: T[] | undefined;
    Object.defineProperty(conversation, 'messages', {
      enumerable: true,
      get: () => (messages ??= read().map(fromDisk)),
    });
    return conversation;
  };
  const conversations: ChatConversation<T>[] = [];
  try {
    const raw = localStorage.getItem(CONVERSATIONS_STORAGE_KEY);
    if (raw) {
      const saved = JSON.parse(raw);
      if ((saved.version === 1 || saved.version === 2) && Array.isArray(saved.conversations)) {
        for (const c of saved.conversations) {
          if (
            typeof c?.id !== 'string' ||
            typeof c.serverId !== 'string' ||
            typeof c.sessionKey !== 'string' ||
            typeof c.title !== 'string' ||
            !Number.isFinite(c.createdAt) ||
            !Number.isFinite(c.updatedAt)
          )
            continue;
          const id = c.id;
          if (saved.version === 1 || Array.isArray(c.messages)) {
            const messages = readMessages(c.messages);
            pending.set(c.id, messages);
            conversations.push(
              lazy({ ...c, messages: [], messageCount: countDiskMessages(messages) }, () =>
                readBody(id)
              )
            );
          } else {
            stored.add(c.id);
            conversations.push(
              lazy(
                {
                  ...c,
                  messages: [],
                  messageCount:
                    Number.isInteger(c.messageCount) && c.messageCount >= 0
                      ? c.messageCount
                      : countDiskMessages(readBody(id)),
                },
                () => readBody(id)
              )
            );
          }
        }
        if (conversations.length) {
          const active =
            conversations.find((c) => c.id === saved.activeConversationId) ??
            [...conversations].sort((a, b) => b.updatedAt - a.updatedAt)[0];
          return { conversations, activeConversationId: active.id };
        }
      }
    }
  } catch {
    // A damaged index must not prevent the old history from being recovered.
  }
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (!key?.startsWith(LEGACY_PREFIX)) continue;
      try {
        const diskMessages = readMessages(JSON.parse(localStorage.getItem(key) || '[]'));
        if (!diskMessages.length) continue;
        const serverId = key.slice(LEGACY_PREFIX.length);
        pending.set(`legacy-${serverId}`, diskMessages);
        conversations.push(
          lazy(
            {
              id: `legacy-${serverId}`,
              serverId,
              sessionKey: serverId,
              title:
                diskMessages
                  .find((m) => m.role === 'user')
                  ?.content.trim()
                  .split('\n')[0]
                  .slice(0, 32) || '',
              createdAt: Date.now(),
              updatedAt: Date.now(),
              messages: [],
              messageCount: countDiskMessages(diskMessages),
            },
            () => readBody(`legacy-${serverId}`)
          )
        );
      } catch {
        // Recover other servers even if one old history is damaged.
      }
    }
  } catch {
    // Storage can be unavailable; keep the conversation usable in memory.
  }
  let active = conversations.find((c) => c.serverId === 'local');
  if (!active) {
    active = newConversation<T>('local');
    conversations.push(active);
  }
  return { conversations, activeConversationId: active.id };
}

export function useChatPersistence<T>({
  toDisk,
  fromDisk,
  debounceMs = 800,
  countMessage,
}: {
  toDisk: (msg: T) => DiskMsg | null;
  fromDisk: (msg: DiskMsg) => T;
  debounceMs?: number;
  countMessage?: (message: T) => boolean;
}) {
  const [storageState] = useState(() => {
    const pending = new Map<string, DiskMsg[]>();
    const stored = new Set<string>();
    return { pending, stored, initial: loadConversations(fromDisk, pending, stored) };
  });
  const changedBodies = useRef(new Set<string>());
  const [state, setState] = useState(storageState.initial);
  const stateRef = useRef(state);
  const dirtyRef = useRef(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const writeStored = useCallback(
    (value: ConversationState<T>) => {
      const bodies = new Map<string, DiskMsg[]>();
      for (const c of value.conversations) {
        if (changedBodies.current.has(c.id) || !storageState.stored.has(c.id)) {
          bodies.set(
            c.id,
            changedBodies.current.has(c.id) || !storageState.pending.has(c.id)
              ? c.messages.map(toDisk).filter((m) => m !== null)
              : storageState.pending.get(c.id)!
          );
        }
      }
      const writeIndex = () =>
        localStorage.setItem(
          CONVERSATIONS_STORAGE_KEY,
          JSON.stringify({
            version: 2,
            activeConversationId: value.activeConversationId,
            conversations: value.conversations.map((c) => ({
              id: c.id,
              serverId: c.serverId,
              sessionKey: c.sessionKey,
              title: c.title,
              createdAt: c.createdAt,
              updatedAt: c.updatedAt,
              ...(c.contextSize ? { contextSize: c.contextSize } : {}),
              messageCount: bodies.has(c.id)
                ? countDiskMessages(bodies.get(c.id)!)
                : (c.messageCount ?? c.messages.length),
              // A partially migrated index retains every not-yet-written body.
              ...(!storageState.stored.has(c.id) && { messages: bodies.get(c.id) }),
            })),
          })
        );
      // Migrate one body and commit a smaller index at a time. Peak storage is
      // the old index plus one body, rather than two copies of all history.
      const migrating = storageState.pending.size > 0;
      const order = [...bodies.keys()].sort(
        (left, right) =>
          Number(right === value.activeConversationId) - Number(left === value.activeConversationId)
      );
      for (const id of order) {
        localStorage.setItem(conversationBodyKey(id), JSON.stringify(bodies.get(id)));
        storageState.stored.add(id);
        try {
          if (migrating) writeIndex();
        } catch (error) {
          storageState.stored.delete(id);
          throw error;
        }
        storageState.pending.delete(id);
      }
      if (!migrating || order.length === 0) writeIndex();
      for (const id of bodies.keys()) changedBodies.current.delete(id);
    },
    [toDisk, storageState]
  );

  const flush = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = null;
    if (!dirtyRef.current) return;
    try {
      writeStored(stateRef.current);
      dirtyRef.current = false;
    } catch {
      // Preserve cached rows and retry on the next change; no blocking dialog.
    }
  }, [writeStored]);

  const update = useCallback((next: ConversationState<T>) => {
    stateRef.current = next;
    dirtyRef.current = true;
    setState(next);
  }, []);

  useEffect(() => {
    if (dirtyRef.current) timerRef.current = setTimeout(flush, debounceMs);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [state, debounceMs, flush]);

  useEffect(() => {
    window.addEventListener('pagehide', flush);
    window.addEventListener('beforeunload', flush);
    return () => {
      window.removeEventListener('pagehide', flush);
      window.removeEventListener('beforeunload', flush);
      flush();
    };
  }, [flush]);

  const setMessages = useCallback(
    (value: T[] | ((prev: T[]) => T[])) => {
      const prev = stateRef.current;
      const active = prev.conversations.find((c) => c.id === prev.activeConversationId)!;
      const messages = typeof value === 'function' ? value(active.messages) : value;
      if (messages === active.messages) return;
      const messageCount = messages.reduce((total, message) => {
        if (countMessage) return total + Number(countMessage(message));
        const disk = toDisk(message);
        return total + Number(!!disk && disk.role !== 'system' && disk.role !== 'tool');
      }, 0);
      let title = '';
      for (const message of messages) {
        const diskMessage = toDisk(message);
        if (diskMessage?.role !== 'user') continue;
        title = diskMessage.content.trim().split('\n')[0].slice(0, 32);
        break;
      }
      changedBodies.current.add(active.id);
      update({
        ...prev,
        conversations: prev.conversations.map((c) =>
          c.id === active.id ? { ...c, messages, messageCount, title, updatedAt: Date.now() } : c
        ),
      });
    },
    [toDisk, update, countMessage]
  );

  const updateActiveConversation = useCallback(
    (patch: Partial<ChatConversation<T>>) => {
      const prev = stateRef.current;
      const active = prev.conversations.find((c) => c.id === prev.activeConversationId);
      if (!active) return;
      update({
        ...prev,
        conversations: prev.conversations.map((conversation) =>
          conversation.id === active.id
            ? { ...conversation, ...patch, updatedAt: Date.now() }
            : conversation
        ),
      });
    },
    [update]
  );

  const selectConversation = useCallback(
    (id: string) => {
      const prev = stateRef.current;
      if (id === prev.activeConversationId || !prev.conversations.some((c) => c.id === id)) return;
      flush();
      update({ ...prev, activeConversationId: id });
      flush();
    },
    [flush, update]
  );

  const createConversation = useCallback(
    (serverId: string) => {
      flush();
      const c = newConversation<T>(serverId);
      update({ activeConversationId: c.id, conversations: [c, ...stateRef.current.conversations] });
      flush();
    },
    [flush, update]
  );

  const selectServer = useCallback(
    (serverId: string) => {
      const existing = stateRef.current.conversations
        .filter((c) => c.serverId === serverId)
        .sort((a, b) => b.updatedAt - a.updatedAt)[0];
      if (existing) selectConversation(existing.id);
      else createConversation(serverId);
    },
    [selectConversation, createConversation]
  );

  const deleteConversation = useCallback(
    async (id: string, cleanup?: () => Promise<void>) => {
      const prev = stateRef.current;
      const deleted = prev.conversations.find((c) => c.id === id);
      if (!deleted) return;
      flush();
      // Finish any partial migration before changing membership, so a failed
      // body write cannot commit a deletion before backend cleanup.
      if (storageState.pending.size) writeStored(prev);
      const conversations = prev.conversations.filter((c) => c.id !== id);
      let activeConversationId = prev.activeConversationId;
      if (id === activeConversationId) {
        let active = conversations
          .filter((c) => c.serverId === deleted.serverId)
          .sort((a, b) => b.updatedAt - a.updatedAt)[0];
        if (!active) {
          active = newConversation<T>(deleted.serverId);
          conversations.unshift(active);
        }
        activeConversationId = active.id;
      }
      const next = { conversations, activeConversationId };
      const discardNewBodies = () => {
        for (const c of conversations) {
          if (prev.conversations.some((previous) => previous.id === c.id)) continue;
          try {
            localStorage.removeItem(conversationBodyKey(c.id));
          } catch {
            /* Retry on a future cleanup. */
          }
          storageState.stored.delete(c.id);
        }
      };
      try {
        writeStored(next);
      } catch (error) {
        discardNewBodies();
        throw error;
      }
      if (cleanup) {
        try {
          await cleanup();
        } catch (error) {
          try {
            writeStored(prev);
            discardNewBodies();
          } catch {
            dirtyRef.current = true;
          }
          throw error;
        }
      }
      update(next);
      dirtyRef.current = false;
      // Retain the body through backend cleanup so failure can restore the index.
      try {
        localStorage.removeItem(conversationBodyKey(id));
      } catch {
        /* Harmless orphan. */
      }
      storageState.stored.delete(id);
      storageState.pending.delete(id);
      changedBodies.current.delete(id);
    },
    [flush, writeStored, update, storageState]
  );

  const activeConversation = state.conversations.find((c) => c.id === state.activeConversationId)!;
  return {
    activeConversation,
    activeConversationId: state.activeConversationId,
    messages: activeConversation.messages,
    conversations: state.conversations
      .filter((c) => c.serverId === activeConversation.serverId)
      .sort((a, b) => b.updatedAt - a.updatedAt),
    setMessages,
    updateActiveConversation,
    selectConversation,
    createConversation,
    selectServer,
    deleteConversation,
  };
}
