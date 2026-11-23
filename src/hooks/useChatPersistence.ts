// Local conversation history. Model context is persisted separately by the backend.
import { useState, useRef, useEffect, useCallback } from 'react';
import type { BubbleChip } from '../components/chat/ChatBubble';
import type { AssistantReplySummary } from '../api/types';

export interface DiskMsg {
  role: string;
  content: string;
  chips?: BubbleChip[];
  replySummary?: AssistantReplySummary;
  incomplete?: boolean;
}

export interface ChatConversation<T> {
  id: string;
  serverId: string;
  sessionKey: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: T[];
  userMessageCount?: number;
  /** Token context window chosen before the first message; absent on legacy history. */
  contextSize?: number;
}

interface ConversationState<T> {
  activeConversationId: string;
  conversations: ChatConversation<T>[];
  defaultContextSize: number;
}

export const CONVERSATIONS_STORAGE_KEY = 'echobird:agent:conversations';
export const conversationBodyKey = (id: string) => `echobird:agent:conversation:${id}`;
const LEGACY_PREFIX = 'echobird:chat:agent_';
const DEFAULT_CONTEXT_SIZE = 32768;
export const CHAT_CONTEXT_OPTIONS = [
  8192,
  16384,
  DEFAULT_CONTEXT_SIZE,
  65536,
  131072,
  262144,
  524288,
  1048576,
] as const;

function isContextSize(value: unknown): value is number {
  return CHAT_CONTEXT_OPTIONS.some((option) => option === value);
}

function newConversation<T>(serverId: string, contextSize: number): ChatConversation<T> {
  const id = crypto.randomUUID();
  return {
    id,
    serverId,
    sessionKey: id,
    title: '',
    createdAt: Date.now(),
    updatedAt: Date.now(),
    messages: [],
    userMessageCount: 0,
    contextSize,
  };
}

function readMessages(value: unknown): DiskMsg[] {
  return Array.isArray(value)
    ? value.filter(
        (m): m is DiskMsg => typeof m?.role === 'string' && typeof m?.content === 'string'
      )
    : [];
}

const countDiskUserMessages = (messages: DiskMsg[]) =>
  messages.reduce((count, message) => count + Number(message.role === 'user'), 0);

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
    let userMessageCount = conversation.userMessageCount;
    Object.defineProperties(conversation, {
      messages: {
        enumerable: true,
        get: () => {
          if (!messages) {
            const body = read();
            userMessageCount ??= countDiskUserMessages(body);
            messages = body.map(fromDisk);
          }
          return messages;
        },
      },
      userMessageCount: {
        enumerable: true,
        // Older indexes have a count of both sides. Read their body only when needed.
        get: () => (userMessageCount ??= countDiskUserMessages(read())),
      },
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
              lazy({ ...c, messages: [], userMessageCount: countDiskUserMessages(messages) }, () =>
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
                  userMessageCount:
                    Number.isInteger(c.userMessageCount) && c.userMessageCount >= 0
                      ? c.userMessageCount
                      : undefined,
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
          const savedDefaultContextSize: unknown = saved.defaultContextSize;
          let defaultContextSize: number;
          if (isContextSize(savedDefaultContextSize)) {
            defaultContextSize = savedDefaultContextSize;
          } else {
            // Older indexes only recorded manual changes on the conversations themselves.
            const latestCustom = conversations.reduce<ChatConversation<T> | undefined>(
              (latest, conversation) =>
                isContextSize(conversation.contextSize) &&
                conversation.contextSize !== DEFAULT_CONTEXT_SIZE &&
                conversation.updatedAt > (latest?.updatedAt ?? -Infinity)
                  ? conversation
                  : latest,
              undefined
            );
            defaultContextSize = latestCustom?.contextSize ?? DEFAULT_CONTEXT_SIZE;
          }
          return { conversations, activeConversationId: active.id, defaultContextSize };
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
              userMessageCount: countDiskUserMessages(diskMessages),
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
    active = newConversation<T>('local', DEFAULT_CONTEXT_SIZE);
    conversations.push(active);
  }
  return {
    conversations,
    activeConversationId: active.id,
    defaultContextSize: DEFAULT_CONTEXT_SIZE,
  };
}

export function useChatPersistence<T>({
  toDisk,
  fromDisk,
  debounceMs = 800,
  countUserMessage,
}: {
  toDisk: (msg: T) => DiskMsg | null;
  fromDisk: (msg: DiskMsg) => T;
  debounceMs?: number;
  countUserMessage?: (message: T) => boolean;
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
            defaultContextSize: value.defaultContextSize,
            conversations: value.conversations.map((c) => ({
              id: c.id,
              serverId: c.serverId,
              sessionKey: c.sessionKey,
              title: c.title,
              createdAt: c.createdAt,
              updatedAt: c.updatedAt,
              ...(c.contextSize ? { contextSize: c.contextSize } : {}),
              userMessageCount: bodies.has(c.id)
                ? countDiskUserMessages(bodies.get(c.id)!)
                : c.userMessageCount,
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
      const userMessageCount = messages.reduce(
        (total, message) =>
          total + Number(countUserMessage?.(message) ?? toDisk(message)?.role === 'user'),
        0
      );
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
          c.id === active.id
            ? { ...c, messages, userMessageCount, title, updatedAt: Date.now() }
            : c
        ),
      });
    },
    [toDisk, update, countUserMessage]
  );

  const setContextSize = useCallback(
    (size: number) => {
      if (!isContextSize(size)) return;
      const prev = stateRef.current;
      const active = prev.conversations.find((c) => c.id === prev.activeConversationId);
      if (!active || active.contextSize === undefined || active.messages.length > 0) return;
      if (size === active.contextSize && size === prev.defaultContextSize) return;
      update({
        ...prev,
        defaultContextSize: size,
        conversations: prev.conversations.map((conversation) =>
          conversation.id === active.id
            ? { ...conversation, contextSize: size, updatedAt: Date.now() }
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
      const prev = stateRef.current;
      const c = newConversation<T>(serverId, prev.defaultContextSize);
      update({ ...prev, activeConversationId: c.id, conversations: [c, ...prev.conversations] });
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
          active = newConversation<T>(deleted.serverId, prev.defaultContextSize);
          conversations.unshift(active);
        }
        activeConversationId = active.id;
      }
      const next = { ...prev, conversations, activeConversationId };
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
    setContextSize,
    selectConversation,
    createConversation,
    selectServer,
    deleteConversation,
  };
}
