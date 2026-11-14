import { useModelRepository } from '../../hooks/useModelRepository';
import { isSmartRouterId } from '../../utils/smartRouter';
import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useI18n } from '../../hooks/useI18n';
import { listen } from '@tauri-apps/api/event';
import * as api from '../../api/tauri';
import type { AgentEvent, ParasiteEvent, ReasoningEffort } from '../../api/types';
import { useChatPersistence } from '../../hooks/useChatPersistence';
import type { DiskMsg } from '../../hooks/useChatPersistence';
import { errorToKey } from '../../utils/normalizeError';
import type { BubbleChip } from '../../components/chat/ChatBubble';
import type { ChatMessage } from './types';
import { MotherAgentContext } from './context';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { useFreeModels } from '../FreeModels/provider';
import { useConfirm } from '../../components/ConfirmDialog';
import { useToast } from '../../components/Toast';
import { getReasoningLevels, loadReasoningPreferences, REASONING_STORAGE_KEY } from './reasoning';

export function selectAgentProtocol(baseUrl: string, anthropicUrl?: string) {
  const useAnthropic = !baseUrl && !!anthropicUrl;
  return {
    provider: useAnthropic ? 'anthropic' : 'openai',
    anthropicUrl: useAnthropic ? anthropicUrl : undefined,
  } as const;
}

// ===== Provider =====

const countConversationMessage = (message: ChatMessage) =>
  message.type === 'user' || message.type === 'assistant';

export function MotherAgentProvider({ children }: { children: React.ReactNode }) {
  // From stores (replaces drilled props)
  const { detectedTools: _detectedTools } = useToolsStore();
  const {
    activePage,
    motherPrefill: initialMessage,
    motherPrefillCursor,
    clearMotherPrefill,
  } = useNavigationStore();
  const onAgentRunningChange = useNavigationStore((s) => s.setAgentRunning);
  const { t, locale } = useI18n(); // locale for agent hint; t for error messages
  const confirm = useConfirm();
  const { showToast } = useToast();
  const { routerEnabled } = useFreeModels();
  const modelDirectory = useModelRepository();
  const savedModels = modelDirectory.models;
  const models = useMemo(
    () => savedModels.filter((model) => !isSmartRouterId(model.internalId) || routerEnabled),
    [savedModels, routerEnabled]
  );
  const [agentModel, setAgentModelRaw] = useState<string | null>(() =>
    localStorage.getItem('echobird_agent_model')
  );
  const setAgentModel = useCallback((v: string | null) => {
    setAgentModelRaw(v);
    if (v) localStorage.setItem('echobird_agent_model', v);
    else localStorage.removeItem('echobird_agent_model');
  }, []);
  useEffect(() => {
    if (!routerEnabled && isSmartRouterId(agentModel)) {
      // Drop the saved selection when the shared router switch makes it unavailable.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setAgentModel(null);
    }
  }, [routerEnabled, agentModel, setAgentModel]);
  const [chatInput, setChatInput] = useState('');
  const runningRef = useRef<{
    sessionKey: string;
    engine: string;
    conversationId?: string;
    requestId: string;
    reasoningEffort?: ReasoningEffort;
  } | null>(null);
  const resettingRef = useRef(false);
  const [isResetting, setIsResetting] = useState(false);

  // Parasite mode — when non-null, sendMessage delegates this turn to the
  // installed Claude Code CLI ('claudecode' is the only supported id today)
  // instead of EchoBird's own agent_loop. Persisted so the user keeps
  // their choice across reloads.
  const [parasiteAgent, setParasiteAgentRaw] = useState<string | null>(() =>
    localStorage.getItem('echobird_parasite_agent')
  );
  const setParasiteAgent = useCallback((id: string | null) => {
    setParasiteAgentRaw(id);
    if (id) localStorage.setItem('echobird_parasite_agent', id);
    else localStorage.removeItem('echobird_parasite_agent');
  }, []);
  const [parasiteAvailable, setParasiteAvailable] = useState<string[]>([]);
  useEffect(() => {
    api
      .parasiteListInstalled()
      .then((ids) => {
        setParasiteAvailable(ids);
        // If the persisted choice is no longer installed (e.g. user
        // uninstalled Claude Code between sessions), clear it so we don't
        // try to spawn a missing binary on the next turn.
        setParasiteAgentRaw((prev) => (prev && !ids.includes(prev) ? null : prev));
      })
      .catch(() => setParasiteAvailable([]));
  }, []);

  const [isProcessing, setIsProcessing] = useState(false);
  const [reasoningPreferences, setReasoningPreferences] = useState(loadReasoningPreferences);
  const reasoningModel = models.find((m) => m.internalId === agentModel);
  const reasoningLevels = parasiteAgent ? [] : getReasoningLevels(reasoningModel);
  // Include the actual model and endpoint, so editing a saved entry cannot reuse
  // a previous provider's preference for a different model under the same ID.
  const reasoningKey = reasoningModel
    ? JSON.stringify([
        reasoningModel.internalId,
        reasoningModel.modelId || reasoningModel.name,
        reasoningModel.baseUrl,
        reasoningModel.anthropicUrl || '',
      ])
    : '';
  const savedEffort = reasoningPreferences[reasoningKey];
  const reasoningEffort = reasoningLevels.includes(savedEffort) ? savedEffort : undefined;
  const setReasoningEffort = (effort: ReasoningEffort | undefined) => {
    if (resettingRef.current || !reasoningLevels.length) return;
    if (effort && !reasoningLevels.includes(effort)) return;
    const next = { ...reasoningPreferences };
    if (effort) next[reasoningKey] = effort;
    else delete next[reasoningKey];
    localStorage.setItem(REASONING_STORAGE_KEY, JSON.stringify(next));
    setReasoningPreferences(next);
  };
  const [agentState, setAgentState] = useState('idle');
  const [chatInputFocused, setChatInputFocused] = useState(false);
  const [chatCursorPos, setChatCursorPos] = useState(0);
  const chatEndRef = useRef<HTMLDivElement>(null!);
  const chatInputRef = useRef<HTMLTextAreaElement>(null!);
  const chatPrefillRef = useRef<{ text: string; cursor: number } | null>(null);
  // Holds the prefill-focus timer so it survives the re-render that
  // clearMotherPrefill() triggers mid-effect — an effect cleanup would
  // tear it down before it fires, dropping the input focus.
  const prefillFocusRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const abortTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const finishRun = useCallback(() => {
    if (abortTimeoutRef.current) clearTimeout(abortTimeoutRef.current);
    abortTimeoutRef.current = null;
    runningRef.current = null;
    setIsProcessing(false);
    setAgentState('idle');
  }, []);
  useEffect(
    () => () => {
      if (prefillFocusRef.current) clearTimeout(prefillFocusRef.current);
      if (abortTimeoutRef.current) clearTimeout(abortTimeoutRef.current);
    },
    []
  );

  useEffect(() => {
    if (!initialMessage || activePage !== 'mother') return;
    chatPrefillRef.current = {
      text: initialMessage,
      cursor: motherPrefillCursor ?? initialMessage.length,
    };
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setChatInput(initialMessage);
    // One-shot: consume the prefill so it doesn't linger in the store and
    // re-fire this effect on every later switch back to the Mother page —
    // which would re-fill "安装XX" into an input the user already cleared.
    clearMotherPrefill();
    if (prefillFocusRef.current) clearTimeout(prefillFocusRef.current);
    prefillFocusRef.current = setTimeout(() => {
      const input = chatInputRef.current;
      const prefill = chatPrefillRef.current;
      if (input && !input.disabled && prefill && input.value === prefill.text) {
        input.focus();
        input.setSelectionRange(prefill.cursor, prefill.cursor);
        chatPrefillRef.current = null;
      }
    }, 100);
  }, [activePage, initialMessage, motherPrefillCursor, clearMotherPrefill]);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void listen<{
      source: string;
      error: 'voice.selectionEmpty' | 'voice.selectionTooLong' | null;
    }>('voice-selection-ask', ({ payload }) => {
      if (disposed) return;
      if (payload.error || !payload.source.trim()) {
        showToast('error', t(payload.error ?? 'voice.selectionEmpty'));
        return;
      }
      const question = t('mother.selectionQuestion');
      useNavigationStore
        .getState()
        .goToMother(
          `${question}\n${t('mother.selectionContent')}${payload.source}`,
          question.length
        );
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      stop?.();
    };
  }, [t, showToast]);

  // SSH servers shared state (persisted via backend)
  const [sshServers, setSSHServers] = useState<
    Array<{ id: string; host: string; port: string; username: string; alias?: string }>
  >([]);

  // Load saved SSH servers on mount
  useEffect(() => {
    api
      .loadSSHServers()
      .then((servers) => {
        setSSHServers(
          servers.map((s) => ({
            id: s.id,
            host: s.host,
            port: String(s.port),
            username: s.username,
            alias: s.alias,
          }))
        );
      })
      .catch(() => {});
  }, []);

  const sshMutationPending = useRef(false);
  const addSSHServer = useCallback(
    async (server: {
      id: string;
      host: string;
      port: string;
      username: string;
      password?: string;
      alias?: string;
    }) => {
      if (sshMutationPending.current) return false;
      sshMutationPending.current = true;
      try {
        await api.saveSSHServer(
          server.id,
          server.host,
          parseInt(server.port) || 22,
          server.username,
          server.password || '',
          server.alias ?? ''
        );
        setSSHServers((prev) => [
          ...prev.filter((saved) => saved.id !== server.id),
          {
            id: server.id,
            host: server.host,
            port: server.port,
            username: server.username,
            alias: server.alias,
          },
        ]);
        useNavigationStore.getState().bumpSshServersVersion();
        return true;
      } catch (error) {
        showToast('error', typeof error === 'string' ? error : String(error));
        return false;
      } finally {
        sshMutationPending.current = false;
      }
    },
    [showToast]
  );
  // Shared mapper: ChatMessage → disk format
  const toDisk = useCallback((m: ChatMessage): DiskMsg | null => {
    if (m.type === 'user') return { role: 'user', content: m.text, chips: m.chips };
    if (m.type === 'assistant') {
      // Skip transient status messages — they're not real chat history
      if (m.text.includes('__CONN_RETRY__') || m.text.includes('__CONN_FAILED__')) return null;
      return { role: 'assistant', content: m.text };
    }
    if (m.type === 'tool_call') {
      // Persist tool invocations so the rebuilt timeline matches what the user saw.
      // Coerce 'running' → 'failed' on save: if we're still running when persisted,
      // any later state (done/failed) overwrites this; if the session ends while
      // running, the call was interrupted and 'failed' is the honest state.
      const status = m.status === 'running' ? 'failed' : m.status;
      return {
        role: 'tool',
        content: JSON.stringify({ id: m.id, name: m.name, args: m.args, status, output: m.output }),
      };
    }
    if (m.type === 'error')
      return { role: 'system', content: (m as { i18nKey?: string }).i18nKey || m.text };
    if (m.type === 'cancelled')
      return { role: 'system', content: (m as { i18nKey?: string }).i18nKey || m.text };
    return null;
  }, []);

  // Shared mapper: disk format → ChatMessage
  const fromDisk = useCallback((m: DiskMsg): ChatMessage => {
    if (m.role === 'tool') {
      try {
        const o = JSON.parse(m.content);
        const status: 'running' | 'done' | 'failed' =
          o.status === 'done' || o.status === 'failed' ? o.status : 'failed';
        return {
          type: 'tool_call',
          id: String(o.id ?? ''),
          name: String(o.name ?? ''),
          args: typeof o.args === 'string' ? o.args : '',
          status,
          output: typeof o.output === 'string' ? o.output : undefined,
        };
      } catch {
        // Corrupted entry — fall back to a visible cancelled marker rather than crashing
        return { type: 'cancelled', text: m.content };
      }
    }
    if (m.role === 'system' && m.content === 'error.userCancelled') {
      return { type: 'cancelled', text: '', i18nKey: m.content };
    }
    if (m.role === 'system' && m.content.startsWith('error.')) {
      return { type: 'error', text: '', i18nKey: m.content };
    }
    if (m.role === 'system') {
      return { type: 'cancelled', text: m.content };
    }
    return {
      type: m.role === 'user' ? 'user' : 'assistant',
      text: m.content,
      ...(m.role === 'user' ? { chips: m.chips } : {}),
    } as ChatMessage;
  }, []);

  const persistence = useChatPersistence<ChatMessage>({
    toDisk,
    fromDisk,
    countMessage: countConversationMessage,
  });
  const {
    messages: chatOutput,
    setMessages: setChatOutput,
    activeConversation,
    selectServer: selectStoredServer,
    createConversation: createStoredConversation,
    selectConversation: selectStoredConversation,
    activeConversationId,
    updateActiveConversation,
    deleteConversation: deleteStoredConversation,
    conversations,
  } = persistence;
  const selectedServerId = activeConversation.serverId;
  const sessionKey = activeConversation.sessionKey;
  const contextSize = activeConversation.contextSize;
  const setContextSize = useCallback(
    (size: number) => {
      if (activeConversation.messages.length > 0 || activeConversation.contextSize === undefined)
        return;
      updateActiveConversation({ contextSize: size });
    },
    [activeConversation, updateActiveConversation]
  );

  const selectServer = useCallback(
    (id: string) => {
      if (resettingRef.current || runningRef.current || isProcessing || id === selectedServerId)
        return;
      selectStoredServer(id);
      setChatInput('');
    },
    [isProcessing, selectedServerId, selectStoredServer]
  );
  const createConversation = useCallback(() => {
    if (resettingRef.current || runningRef.current || isProcessing) return;
    createStoredConversation(selectedServerId);
  }, [isProcessing, selectedServerId, createStoredConversation]);
  const selectConversation = useCallback(
    (id: string) => {
      if (resettingRef.current || runningRef.current || isProcessing || id === activeConversationId)
        return;
      selectStoredConversation(id);
    },
    [isProcessing, activeConversationId, selectStoredConversation]
  );
  const removeSSHServer = useCallback(
    async (id: string) => {
      if (resettingRef.current || runningRef.current || isProcessing || sshMutationPending.current)
        return;
      sshMutationPending.current = true;
      try {
        await api.removeSSHServerFromDisk(id);
        setSSHServers((prev) => prev.filter((s) => s.id !== id));
        if (selectedServerId === id) selectStoredServer('local');
        useNavigationStore.getState().bumpSshServersVersion();
      } catch (error) {
        showToast('error', typeof error === 'string' ? error : String(error));
      } finally {
        sshMutationPending.current = false;
      }
    },
    [isProcessing, selectedServerId, selectStoredServer, showToast]
  );

  useEffect(() => {
    let cancelled = false;
    queueMicrotask(() => {
      if (
        !cancelled &&
        modelDirectory.loaded &&
        agentModel &&
        !savedModels.some((model) => model.internalId === agentModel)
      )
        setAgentModel(null);
    });
    return () => {
      cancelled = true;
    };
  }, [modelDirectory.loaded, savedModels, agentModel, setAgentModel]);

  const agentModelData = models.find((m) => m.internalId === agentModel);

  // Notify parent about running state
  useEffect(() => {
    onAgentRunningChange(isProcessing);
  }, [isProcessing, onAgentRunningChange]);

  // Chat scroll is managed by MotherAgentMain so the user can scroll up
  // freely while the agent is streaming.

  const appendAssistantText = useCallback(
    (text: string) => {
      setChatOutput((prev) => {
        const last = prev[prev.length - 1];
        if (last?.type === 'assistant') {
          return [...prev.slice(0, -1), { ...last, text: last.text + text }];
        }
        return [...prev, { type: 'assistant', text }];
      });
    },
    [setChatOutput]
  );

  // Subscribe to parasite events — shape mirrors agent_event so the chat
  // renderer (ChatBubble) needs no special handling. Tool-call surface is
  // intentionally absent in parasite mode: the wrapped CLI agent runs its
  // own tools opaquely, and surfacing them would lie about what executed.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    api
      .listenParasiteEvents((event: ParasiteEvent) => {
        const run = runningRef.current;
        if (cancelled || !run || run.engine !== 'claudecode') return;
        if (event.conversation_id !== run.conversationId) return;
        if (event.request_id !== run.requestId) return;
        switch (event.type) {
          case 'text_delta':
            appendAssistantText(event.text);
            break;
          case 'done':
            finishRun();
            break;
          case 'error': {
            const key = errorToKey(event.message);
            if (key !== 'error.userCancelled') {
              const errBubble = key
                ? { type: 'error' as const, text: '', i18nKey: key }
                : { type: 'error' as const, text: String(event.message ?? '').slice(0, 500) };
              setChatOutput((prev) => [...prev, errBubble]);
            } else {
              // Append a cancellation marker if there isn't already one.
              setChatOutput((prev) => {
                const last = prev[prev.length - 1];
                if (last && last.type === 'cancelled') return prev;
                return [...prev, { type: 'cancelled', text: '', i18nKey: 'error.userCancelled' }];
              });
            }
            finishRun();
            break;
          }
          case 'state':
            setAgentState(event.state);
            break;
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setChatOutput, finishRun, appendAssistantText]);

  // Subscribe to agent events
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    api
      .listenAgentEvents((event: AgentEvent) => {
        const run = runningRef.current;
        if (cancelled || !run || run.engine !== 'mother') return;
        if (event.conversation_id !== run.conversationId) return;
        if (event.request_id !== run.requestId) return;
        switch (event.type) {
          case 'text_delta':
            appendAssistantText(event.text);
            break;
          case 'thinking':
            // Reasoning is private — not surfaced into the chat stream
            break;
          case 'tool_call_start':
            setChatOutput((prev) => [
              ...prev,
              {
                type: 'tool_call',
                id: event.id,
                name: event.name,
                args: '',
                status: 'running',
              },
            ]);
            break;
          case 'tool_call_args':
            setChatOutput((prev) =>
              prev.map((m) =>
                m.type === 'tool_call' && m.id === event.id
                  ? { ...m, args: m.args + event.args }
                  : m
              )
            );
            break;
          case 'tool_result':
            setChatOutput((prev) =>
              prev.map((m) =>
                m.type === 'tool_call' && m.id === event.id
                  ? { ...m, status: event.success ? 'done' : 'failed', output: event.output }
                  : m
              )
            );
            break;
          case 'done':
            finishRun();
            break;
          case 'error': {
            const key = errorToKey(event.message);
            // Skip duplicate cancelled message — already added immediately in abortAgent()
            if (key !== 'error.userCancelled') {
              // v4.7.0+: when the backend emits a provider's verbatim
              // error ("Invalid API Key" / "Rate limit exceeded" / etc.),
              // errorToKey returns null and we render that message
              // straight through. Categorized errors still go via i18n.
              const errBubble = key
                ? { type: 'error' as const, text: '', i18nKey: key }
                : { type: 'error' as const, text: String(event.message ?? '').slice(0, 500) };
              setChatOutput((prev) => [...prev, errBubble]);
            }
            finishRun();
            break;
          }
          case 'state':
            setAgentState(event.state);
            break;
        }
      })
      .then((fn) => {
        if (cancelled) {
          fn(); // Already unmounted, clean up immediately
        } else {
          unlisten = fn;
        }
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // Subscribe-once on mount — the Tauri listener is torn down on unmount.
  }, [setChatOutput, finishRun, appendAssistantText]);

  // Internal send function
  const handleChatSendInternal = useCallback(
    async (message: string, displayText?: string, chips?: BubbleChip[]) => {
      if (resettingRef.current || runningRef.current || isProcessing || !message.trim()) return;
      // Snapshot for this user message. Changes during the run apply on the next send.
      const run = {
        sessionKey,
        engine: parasiteAgent || 'mother',
        conversationId:
          parasiteAgent && activeConversation.id.startsWith('legacy-') ? undefined : sessionKey,
        requestId: crypto.randomUUID(),
        reasoningEffort,
      };
      runningRef.current = run;
      setIsProcessing(true);
      // Use display text + chips if provided (chip-send path), else full message text
      setChatOutput((prev) => [
        ...prev,
        { type: 'user', text: (displayText ?? message).trim(), chips },
      ]);

      // Parasite mode: delegate this turn to the installed Claude Code CLI.
      // CC runs in its own environment with its own memory, skills, tools,
      // AND model config. EchoBird's model selector is hidden while
      // Connect is active — model management belongs to Claude Code
      // (configure via App Desktop or `claude /model`).
      if (parasiteAgent) {
        try {
          await api.parasiteSendMessage({
            agentId: parasiteAgent,
            conversationId: run.conversationId,
            requestId: run.requestId,
            message: message.trim(),
          });
        } catch (e) {
          if (runningRef.current !== run) return;
          const key = errorToKey(String(e));
          const type: 'cancelled' | 'error' = key === 'error.userCancelled' ? 'cancelled' : 'error';
          const bubble: ChatMessage = key
            ? { type, text: '', i18nKey: key }
            : { type, text: String(e ?? '').slice(0, 500) };
          setChatOutput((prev) => [...prev, bubble]);
          finishRun();
        }
        return;
      }

      const modelData = models.find((m) => m.internalId === agentModel);
      if (!modelData) {
        setChatOutput((prev) => [
          ...prev,
          { type: 'error', text: '', i18nKey: 'error.noModelSelected' },
        ]);
        finishRun();
        return;
      }

      try {
        // Protocol is decided by the available endpoints. When both endpoint
        // fields exist, OpenAI is the default because baseUrl is the primary
        // endpoint for providers such as Grok. Anthropic is used only when the
        // model has no OpenAI-compatible baseUrl.
        //
        // The backend snapshots one protocol per request and never switches
        // mid-flight (auto-downgrade was removed in v5.2.0). So fabricating an
        // Anthropic URL from a "/v1" base — as we used to — routes every
        // OpenAI-only provider (OpenRouter, OpenAI, Grok, Groq, Together, …)
        // to a non-existent "/anthropic/v1/messages" endpoint and fails hard
        // with "Not Found" instead of just using the OpenAI path that works.
        const protocol = selectAgentProtocol(modelData.baseUrl, modelData.anthropicUrl);
        const anthropicUrl = protocol.anthropicUrl;
        await api.sendAgentMessage({
          message: message.trim(),
          conversation_id: sessionKey,
          request_id: run.requestId,
          model_id: modelData.internalId,
          base_url: modelData.baseUrl || '',
          api_key: modelData.apiKey,
          model_name: modelData.modelId || modelData.name,
          provider: protocol.provider,
          anthropic_url: anthropicUrl,
          server_ids: selectedServerId === 'local' ? [] : [selectedServerId],
          skills: [],
          locale: locale || undefined,
          ...(run.reasoningEffort ? { reasoning_effort: run.reasoningEffort } : {}),
          ...(contextSize ? { context_size: contextSize } : {}),
        });
      } catch (e) {
        if (runningRef.current !== run) return;
        const key = errorToKey(String(e));
        const type: 'cancelled' | 'error' = key === 'error.userCancelled' ? 'cancelled' : 'error';
        // Same v4.7.0+ pass-through as the streaming-error case: when
        // errorToKey can't classify, render the message verbatim.
        const bubble: ChatMessage = key
          ? { type, text: '', i18nKey: key }
          : { type, text: String(e ?? '').slice(0, 500) };
        setChatOutput((prev) => [...prev, bubble]);
        finishRun();
      }
    },
    [
      agentModel,
      models,
      isProcessing,
      selectedServerId,
      sessionKey,
      activeConversation.id,
      contextSize,
      locale,
      parasiteAgent,
      setChatOutput,
      finishRun,
      reasoningEffort,
    ]
  );

  // Chat send (from input)
  const handleChatSend = useCallback(async () => {
    if (resettingRef.current || runningRef.current || isProcessing || !chatInput.trim()) return;
    const msg = chatInput.trim();
    setChatInput('');
    handleChatSendInternal(msg);
  }, [chatInput, isProcessing, handleChatSendInternal]);

  const deleteConversation = useCallback(
    async (id: string) => {
      if (resettingRef.current || runningRef.current || isProcessing) return;
      const conversation = conversations.find((c) => c.id === id);
      if (!conversation) return;
      resettingRef.current = true;
      try {
        const ok = await confirm({
          title: t('mother.deleteConversationTitle'),
          message: t('mother.deleteConversationConfirm').replace(
            '{title}',
            conversation.title || t('mother.newConversation')
          ),
          confirmText: t('btn.delete'),
          cancelText: t('btn.cancel'),
          type: 'danger',
        });
        if (!ok) return;
        setIsResetting(true);
        await deleteStoredConversation(id, async () => {
          const legacy = conversation.id.startsWith('legacy-');
          await api.resetConversationContext(
            conversation.sessionKey,
            !legacy || conversation.serverId === 'local',
            legacy ? undefined : conversation.sessionKey
          );
        });
        if (id === activeConversationId) setChatInput('');
      } catch (e) {
        const key = errorToKey(String(e));
        showToast('error', key ? t(key) : String(e ?? '').slice(0, 500));
      } finally {
        resettingRef.current = false;
        setIsResetting(false);
      }
    },
    [
      isProcessing,
      conversations,
      activeConversationId,
      confirm,
      t,
      showToast,
      deleteStoredConversation,
    ]
  );

  return (
    <MotherAgentContext.Provider
      value={{
        models,
        agentModel,
        setAgentModel,
        chatInput,
        setChatInput,
        chatOutput,
        conversations: persistence.conversations,
        activeConversationId: persistence.activeConversationId,
        contextSize: activeConversation.contextSize,
        contextSizeEditable:
          activeConversation.contextSize !== undefined && activeConversation.messages.length === 0,
        setContextSize,
        createConversation,
        selectConversation,
        deleteConversation,
        isProcessing: isProcessing || isResetting,
        isResetting,
        agentModelData,
        reasoningLevels,
        reasoningEffort,
        setReasoningEffort,
        agentState,
        chatInputFocused,
        setChatInputFocused,
        chatCursorPos,
        setChatCursorPos,
        chatInputRef,
        chatPrefillRef,
        chatEndRef,
        handleChatSend,
        sendMessage: handleChatSendInternal,

        sshServers,
        addSSHServer,
        removeSSHServer,
        selectedServerId,
        selectServer,
        parasiteAgent,
        setParasiteAgent,
        parasiteAvailable,
        abortAgent: () => {
          const run = runningRef.current;
          if (!run) return;
          // Idempotent on rapid multi-clicks: backend `agent_abort` is
          // safe to invoke repeatedly (a no-op once `sess.running` is
          // false), but each frontend click used to stack another
          // "已取消" chat bubble — users who clicked impatiently 5-10
          // times while waiting for the agent to unwind saw a flood of
          // duplicates. We now skip adding a new bubble when the last
          // chat item is already a `cancelled` one, and skip firing
          // backend abort + restart safety-net timer while one is
          // already pending.
          if (abortTimeoutRef.current) {
            // An abort is already in flight — backend will unwind in a
            // moment, no need to spam more requests or bubbles.
            return;
          }
          if (run.engine !== 'mother') {
            api.parasiteAbort(run.engine, run.conversationId).catch(() => {});
          } else {
            api.abortAgent(run.sessionKey).catch(() => {});
          }
          setChatOutput((o) => {
            const last = o[o.length - 1];
            if (last && last.type === 'cancelled') {
              return o; // dedup: don't stack duplicate cancelled bubbles
            }
            return [...o, { type: 'cancelled', text: '', i18nKey: 'error.userCancelled' }];
          });
          // Frontend safety net: force reset after 3s if backend doesn't respond
          abortTimeoutRef.current = setTimeout(() => {
            finishRun();
          }, 3000);
        },
        maDiskTotal: chatOutput.length,
        loadOlderChat: async () => [],
      }}
    >
      {children}
    </MotherAgentContext.Provider>
  );
}
