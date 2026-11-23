import { isSmartRouterId } from '../../utils/smartRouter';
import React, { useState, useEffect, useLayoutEffect, useRef, useCallback } from 'react';
import { ArrowUp, ChevronDown, Square } from 'lucide-react';
import { RemoteModelSelector, type ModelOption } from '../../components/RemoteModelSelector';
import { getModelIcon } from '../../components/cards/ModelCard';
import { ChatBubble, ToolCallCard } from '../../components/chat';
import { CHAT_CONTEXT_OPTIONS } from '../../hooks/useChatPersistence';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useMotherAgent } from './context';
import { MessageNavigation } from './MessageNavigation';
import { QuickCommandChips } from './QuickCommandChips';
import { ReasoningControl } from './ReasoningControl';
import { MA_PAGE_SIZE, type ChatMessage } from './types';

const PROMPT_SCROLL_INSET = 16;

function isVisibleMessage(message: ChatMessage) {
  return message.type !== 'tool_call' || !message.name.startsWith('memory_');
}

// ===== Main Content (center area) — CHAT =====
export function MotherAgentMain() {
  const { t } = useI18n();
  const {
    models,
    agentModel,
    setAgentModel,
    chatInput,
    setChatInput,
    chatOutput,
    isProcessing,
    turnStartedAt,
    isResetting,
    agentModelData: _agentModelData,
    reasoningLevels,
    reasoningEffort,
    setReasoningEffort,
    agentState: _agentState,
    chatEndRef,
    chatInputRef,
    chatPrefillRef,
    handleChatSend,

    sshServers: _sshServers,
    selectedServerId,
    activeConversationId,
    abortAgent,
    maDiskTotal,
    loadOlderChat,
    parasiteAgent,
    setParasiteAgent,
    parasiteAvailable,
    contextSize,
    contextSizeEditable,
    setContextSize,
  } = useMotherAgent();
  const composingRef = useRef(false);
  // A first-visit chunk can arrive after the provider's prefill focus timer.
  const initialInput = useRef(chatInput);
  useLayoutEffect(() => {
    if (initialInput.current) chatInputRef.current?.focus();
  }, [chatInputRef]);
  useLayoutEffect(() => {
    const input = chatInputRef.current;
    const prefill = chatPrefillRef?.current;
    if (input && !isProcessing && prefill && prefill.text === chatInput) {
      input.focus();
      input.setSelectionRange(prefill.cursor, prefill.cursor);
      chatPrefillRef.current = null;
    }
  }, [chatInput, chatInputRef, chatPrefillRef, isProcessing]);
  const visibleChatOutput = React.useMemo(() => chatOutput.filter(isVisibleMessage), [chatOutput]);

  const prompts = React.useMemo(
    () =>
      visibleChatOutput.flatMap((message, index) =>
        message.type === 'user' ? [{ index, message }] : []
      ),
    [visibleChatOutput]
  );

  // Build model list for RemoteModelSelector (with icons)
  const modelList: ModelOption[] = React.useMemo(
    () =>
      models.map((m) => ({
        id: m.internalId,
        name: m.name,
        icon: getModelIcon('', m.modelId),
        iconKind: isSmartRouterId(m.internalId) ? ('router' as const) : undefined,
        routerIcon: m.routerIcon,
      })),
    [models]
  );

  // Estimated UTF-8 byte footprint of the current chat — shown in the
  // usage ring so users can see how close they are to the
  // agent_loop trim threshold (MAX_CONTEXT_BYTES in echobird_core).
  // estimateContextBytes re-encodes the whole transcript; recomputing it on
  // every streamed token is O(n^2) and only updates a coarse usage estimate, so
  // debounce it (recompute ~300ms after updates settle) instead of per-token.
  const [contextBytes, setContextBytes] = useState(0);
  useEffect(() => {
    const id = setTimeout(() => setContextBytes(estimateContextBytes(chatOutput)), 300);
    return () => clearTimeout(id);
  }, [chatOutput]);

  // ── Scroll management — sticky-bottom auto-follow ──
  // Default-stuck-to-bottom: streaming chunks, tool calls, "thinking" indicators
  // all keep the viewport pinned. Sticky flips off the moment the user scrolls
  // up beyond the threshold; flips back on when they return to the bottom.
  const chatContainerRef = useRef<HTMLDivElement>(null!);
  const stickToBottomRef = useRef(true);
  const [showScrollBtn, setShowScrollBtn] = useState(false);
  const [retainedHistoryStart, setRetainedHistoryStart] = useState<number>();
  const [showSkeleton, setShowSkeleton] = useState(false);
  const paginationGenerationRef = useRef(0);
  const pendingPromptRef = useRef<ChatMessage | null>(null);
  const [readingPromptIndex, setReadingPromptIndex] = useState<number>();
  const activePromptIndex =
    (showScrollBtn ? readingPromptIndex : undefined) ?? prompts[prompts.length - 1]?.index;
  const latestPageStart = Math.max(0, visibleChatOutput.length - MA_PAGE_SIZE);
  const firstRenderedIndex = Math.min(latestPageStart, retainedHistoryStart ?? latestPageStart);

  // Re-snap to bottom every time the user opens this page — not just on first
  // hydration (the component stays mounted via CSS hidden, so a one-shot
  // initial-scroll flag misses subsequent visits).
  const isMotherActive = useNavigationStore((s) => s.activePage === 'mother');

  const snapToBottom = useCallback(() => {
    const c = chatContainerRef.current;
    if (!c) return;
    c.scrollTop = c.scrollHeight;
    setShowScrollBtn(false);
  }, []);

  // Fit drafts before paint, including prefills and clearing after send.
  useLayoutEffect(() => {
    const input = chatInputRef.current;
    if (!input || !isMotherActive) return;
    const resize = (contentChanged: boolean) => {
      const previousScrollTop = input.scrollTop;
      input.style.height = 'auto';
      input.style.height = `${input.scrollHeight}px`;
      const followCaret =
        contentChanged &&
        document.activeElement === input &&
        input.selectionStart === input.value.length &&
        input.selectionEnd === input.value.length;
      input.scrollTop = followCaret ? input.scrollHeight : previousScrollTop;
      if (stickToBottomRef.current) snapToBottom();
    };
    resize(true);
    // Side panels and window resizing can change wrapping without editing the draft.
    let width = input.clientWidth;
    const observer = new ResizeObserver(() => {
      if (input.clientWidth === width) return;
      width = input.clientWidth;
      resize(false);
    });
    observer.observe(input);
    return () => observer.disconnect();
  }, [chatInput, chatInputRef, isMotherActive, snapToBottom]);

  // Reset pagination + re-arm sticky when the conversation changes
  useEffect(() => {
    paginationGenerationRef.current += 1;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setRetainedHistoryStart(undefined);
    setShowSkeleton(false);
    setReadingPromptIndex(undefined);
    pendingPromptRef.current = null;
    stickToBottomRef.current = true;
    requestAnimationFrame(snapToBottom);
  }, [selectedServerId, activeConversationId, snapToBottom]);

  // Re-arm sticky + snap whenever the page becomes active
  useEffect(() => {
    if (!isMotherActive) return;
    stickToBottomRef.current = true;
    // Two rAFs: first lets layout settle after CSS unhide, second scrolls.
    requestAnimationFrame(() => requestAnimationFrame(snapToBottom));
  }, [isMotherActive, snapToBottom]);

  // Sticky auto-follow: every chat update (new message, streaming delta,
  // tool call, processing-flag flip) re-snaps to bottom if sticky is on.
  useLayoutEffect(() => {
    if (!stickToBottomRef.current) return;
    snapToBottom();
  }, [chatOutput, firstRenderedIndex, isProcessing, snapToBottom]);

  // ResizeObserver safety net — catches async growth (markdown re-render,
  // image loads, code-block syntax highlighting) that bypasses React state.
  useEffect(() => {
    const c = chatContainerRef.current;
    if (!c) return;
    const ro = new ResizeObserver(() => {
      if (stickToBottomRef.current) snapToBottom();
    });
    Array.from(c.children).forEach((child) => ro.observe(child));
    return () => ro.disconnect();
  }, [snapToBottom]);

  const scrollToPrompt = useCallback((index: number) => {
    const container = chatContainerRef.current;
    const target = container?.querySelector<HTMLElement>(`[data-prompt-anchor="${index}"]`);
    if (!target) return;
    container.scrollTop = target.offsetTop - PROMPT_SCROLL_INSET;
    stickToBottomRef.current =
      container.scrollHeight - container.scrollTop - container.clientHeight < 80;
    setShowScrollBtn(!stickToBottomRef.current);
    setReadingPromptIndex(index);
  }, []);

  const jumpToPrompt = (message: ChatMessage, index: number) => {
    // A pending prepend must not restore the previous scroll position after a jump.
    paginationGenerationRef.current += 1;
    // Keep the expanded rows mounted as more replies arrive at the end.
    setRetainedHistoryStart(Math.min(firstRenderedIndex, index));
    if (index >= firstRenderedIndex && !showSkeleton) {
      scrollToPrompt(index);
      return;
    }
    pendingPromptRef.current = message;
    stickToBottomRef.current = false;
    setShowSkeleton(false);
  };

  useLayoutEffect(() => {
    const pending = pendingPromptRef.current;
    if (!pending) return;
    pendingPromptRef.current = null;
    const index = visibleChatOutput.indexOf(pending);
    if (index >= firstRenderedIndex) scrollToPrompt(index);
  }, [firstRenderedIndex, scrollToPrompt, showSkeleton, visibleChatOutput]);

  const handleScroll = async () => {
    const container = chatContainerRef.current;
    if (!container) return;
    const distanceFromBottom =
      container.scrollHeight - container.scrollTop - container.clientHeight;
    const isNearBottom = distanceFromBottom < 80;
    // Sticky tracks the actual scroll position. Programmatic snaps land
    // at distance≈0, so they correctly keep sticky=true; user wheel/drag
    // away from the bottom flips it off.
    stickToBottomRef.current = isNearBottom;
    setShowScrollBtn(!isNearBottom && visibleChatOutput.length > 0);

    if (!isNearBottom && prompts.length > 1) {
      // Each anchor is positioned relative to the scroll viewport.
      const anchors = container.querySelectorAll<HTMLElement>('[data-prompt-anchor]');
      const readingTop = container.scrollTop + PROMPT_SCROLL_INSET + 4;
      let low = 0;
      let high = anchors.length;
      while (low < high) {
        const middle = (low + high) >>> 1;
        if (anchors[middle].offsetTop <= readingTop) low = middle + 1;
        else high = middle;
      }
      const index = anchors[Math.max(0, low - 1)]?.dataset.promptAnchor;
      setReadingPromptIndex(index === undefined ? undefined : Number(index));
    }

    if (container.scrollTop !== 0 || showSkeleton) return;
    const generation = paginationGenerationRef.current;

    // Phase 1: more in-memory messages to show
    if (firstRenderedIndex > 0) {
      setShowSkeleton(true);
      const prevScrollHeight = container.scrollHeight;
      setTimeout(() => {
        if (generation !== paginationGenerationRef.current) return;
        setShowSkeleton(false);
        setRetainedHistoryStart(Math.max(0, firstRenderedIndex - MA_PAGE_SIZE));
        requestAnimationFrame(() => {
          if (generation === paginationGenerationRef.current && chatContainerRef.current) {
            chatContainerRef.current.scrollTop =
              chatContainerRef.current.scrollHeight - prevScrollHeight;
          }
        });
      }, 300);
      return;
    }

    // Phase 2: load older batch from disk when in-memory is exhausted
    const alreadyLoaded = chatOutput.length;
    if (alreadyLoaded >= maDiskTotal) return;

    setShowSkeleton(true);
    const prevScrollHeight2 = container.scrollHeight;
    try {
      let loaded = alreadyLoaded;
      let visibleLoaded = 0;
      while (loaded < maDiskTotal) {
        const older = await loadOlderChat();
        if (generation !== paginationGenerationRef.current) return;
        if (older.length === 0) break;
        loaded += older.length;
        visibleLoaded = older.filter(isVisibleMessage).length;
        if (visibleLoaded > 0) break;
      }
      setShowSkeleton(false);
      if (visibleLoaded === 0) return;
      setRetainedHistoryStart(0);
      requestAnimationFrame(() => {
        if (generation === paginationGenerationRef.current && chatContainerRef.current) {
          chatContainerRef.current.scrollTop =
            chatContainerRef.current.scrollHeight - prevScrollHeight2;
        }
      });
    } catch {
      if (generation !== paginationGenerationRef.current) return;
      setShowSkeleton(false);
    }
  };

  const scrollToBottom = useCallback(() => {
    stickToBottomRef.current = true;
    setShowScrollBtn(false);
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [chatEndRef]);

  const showWelcome = visibleChatOutput.length === 0 && !isProcessing;
  const lastVisibleOutput = visibleChatOutput[visibleChatOutput.length - 1];

  return (
    <div className="flex flex-col h-full">
      {/* Chat conversation area */}
      <div className="relative flex-1">
        <div
          ref={chatContainerRef}
          onScroll={handleScroll}
          className="absolute inset-0 overflow-y-auto slim-scroll py-4"
        >
          {showWelcome ? (
            <div className="grid min-h-full grid-rows-[minmax(0,0.35fr)_auto_minmax(0,0.65fr)]">
              <div className="row-start-2 flex flex-col items-center gap-7">
                <div className="flex items-center gap-3">
                  <img
                    src="/brand/bird.png"
                    alt=""
                    aria-hidden="true"
                    draggable={false}
                    className="h-12 w-12 select-none"
                  />
                  <span className="brand-mark text-cyber-text" style={{ fontSize: 36 }}>
                    {t('app.name')}
                  </span>
                </div>
                <div className="w-full">
                  <QuickCommandChips />
                </div>
              </div>
            </div>
          ) : (
            <div className="mother-transcript py-2">
              {/* Skeleton placeholders — shown briefly when lazy-loading older messages */}
              {showSkeleton &&
                [0, 1, 2].map((i) => <ChatBubble key={`sk-${i}`} role="skeleton" content="" />)}
              {visibleChatOutput.slice(firstRenderedIndex).map((msg, offset) => {
                const i = firstRenderedIndex + offset;
                if (msg.type === 'user') {
                  return (
                    <div key={i} data-prompt-anchor={i}>
                      <ChatBubble role="user" content={msg.text} chips={msg.chips} />
                    </div>
                  );
                }
                if (msg.type === 'tool_call') {
                  return (
                    <ToolCallCard
                      key={`${i}-${msg.id}`}
                      name={msg.name}
                      args={msg.args}
                      status={msg.status}
                      liveOutput={msg.liveOutput}
                      output={msg.output}
                    />
                  );
                }
                if (msg.type === 'assistant') {
                  const isCurrentResponse = msg === lastVisibleOutput;
                  const nextMessage = visibleChatOutput[i + 1];
                  const legacyReply =
                    !msg.incomplete && (!nextMessage || nextMessage.type === 'user');
                  return (
                    <ChatBubble
                      key={msg.messageId}
                      role="assistant"
                      content={msg.text}
                      isStreaming={isProcessing && isCurrentResponse}
                      workingStartedAt={isCurrentResponse ? turnStartedAt : undefined}
                      replySummary={msg.replySummary ?? (legacyReply ? null : undefined)}
                    />
                  );
                }
                if (msg.type === 'cancelled') {
                  const text = msg.i18nKey
                    ? t(msg.i18nKey as import('../../i18n/types').TKey)
                    : msg.text;
                  return (
                    <div key={i} className="flex justify-center my-4">
                      <span className="text-cyber-text-muted/35 text-xs font-mono">{text}</span>
                    </div>
                  );
                }
                if (msg.type === 'error') {
                  const text = msg.i18nKey
                    ? t(msg.i18nKey as import('../../i18n/types').TKey)
                    : msg.text;
                  return <ChatBubble key={i} role="error" content={text} />;
                }
                return null;
              })}
              {turnStartedAt !== undefined && lastVisibleOutput?.type !== 'assistant' && (
                <ChatBubble role="working" content="" workingStartedAt={turnStartedAt} />
              )}
            </div>
          )}
          {!showWelcome && <div ref={chatEndRef} />}
        </div>
        {prompts.length > 0 && (
          <MessageNavigation
            key={activeConversationId}
            prompts={prompts}
            activeIndex={activePromptIndex}
            active={isMotherActive}
            onJump={jumpToPrompt}
          />
        )}
        {/* Scroll to bottom button */}
        {showScrollBtn && (
          <button
            onClick={scrollToBottom}
            className="absolute bottom-3 right-3 w-7 h-7 flex items-center justify-center bg-cyber-bg/90 border border-cyber-border/50 rounded text-cyber-text-secondary hover:text-cyber-text hover:border-cyber-border/50 transition-colors z-10"
          >
            <ChevronDown className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Rich input area — Claude-style elevated rounded card */}
      <div className="flex-shrink-0 mt-1 mb-1">
        <div className="mother-composer rounded-[26px] p-2.5">
          <textarea
            ref={chatInputRef}
            value={chatInput}
            onChange={(e) => {
              if (chatPrefillRef) chatPrefillRef.current = null;
              setChatInput(e.target.value);
            }}
            onCompositionStart={() => {
              composingRef.current = true;
            }}
            onCompositionEnd={() => {
              composingRef.current = false;
            }}
            onBlur={() => {
              composingRef.current = false;
            }}
            onKeyDown={(e) => {
              // WebKit can end composition before the confirming Enter keydown.
              // Its legacy 229 code still identifies that IME event.
              if (
                composingRef.current ||
                e.nativeEvent?.isComposing ||
                e.nativeEvent?.keyCode === 229
              ) {
                return;
              }
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                if (!isProcessing) handleChatSend();
              }
            }}
            placeholder={t('mother.enterMessage')}
            disabled={isProcessing}
            rows={2}
            style={{ minHeight: 48, maxHeight: 'max(48px, calc(50vh - 60px))', overflowY: 'auto' }}
            className="w-full bg-transparent px-2 py-1 text-sm text-cyber-text font-sans font-medium outline-none placeholder:text-cyber-text-muted disabled:opacity-30 resize-none"
          />
          <div className="mother-composer-controls flex items-center justify-end gap-1">
            <RemoteModelSelector
              showMenuIcons
              models={modelList}
              currentModelId={parasiteAgent || agentModel}
              loading={false}
              onSelect={(id) => {
                if (id === PARASITE_CLAUDE_ID) {
                  // Switch to parasite mode. Intentionally don't clear
                  // agentModel so the user can flip back to their previous
                  // regular model later without having to re-select it.
                  setParasiteAgent(PARASITE_CLAUDE_ID);
                } else {
                  setAgentModel(id || null);
                  setParasiteAgent(null);
                }
              }}
              placeholder={t('mother.selectModel')}
              extras={[
                {
                  id: PARASITE_CLAUDE_ID,
                  name: 'Claude Code',
                  // CLI-style blocky icon — deliberately different from
                  // claudecode.svg (the Anthropic star logo used by the
                  // claude-opus-* models) so the parasite engine row is
                  // visually distinct from regular Claude models.
                  icon: '/icons/tools/claude.svg',
                  disabled: !parasiteAvailable.includes(PARASITE_CLAUDE_ID),
                  disabledLabel: t('status.notInstalled'),
                },
              ]}
            />
            <ReasoningControl
              key={JSON.stringify([
                parasiteAgent || agentModel,
                _agentModelData?.modelId || _agentModelData?.name,
                _agentModelData?.baseUrl,
                _agentModelData?.anthropicUrl,
                _agentModelData?.scope,
                activeConversationId,
                isResetting,
                isMotherActive,
              ])}
              levels={reasoningLevels || []}
              value={reasoningEffort}
              disabled={isResetting || !isMotherActive}
              onChange={setReasoningEffort}
            />
            {/* This estimate belongs to EchoBird's own context loop. */}
            {parasiteAgent !== PARASITE_CLAUDE_ID && (
              <ContextUsage
                contextBytes={contextBytes}
                contextSize={contextSize}
                editable={contextSizeEditable}
                onContextSizeChange={setContextSize}
              />
            )}
            {isProcessing ? (
              <button
                onClick={() => abortAgent()}
                className="w-8 h-8 shrink-0 rounded-lg flex items-center justify-center bg-red-500/20 hover:bg-red-500/30 transition-colors"
              >
                <Square size={14} fill="#f87171" className="text-red-400" />
              </button>
            ) : (
              <button
                onClick={handleChatSend}
                disabled={!chatInput.trim()}
                className="w-8 h-8 shrink-0 rounded-lg flex items-center justify-center bg-cyber-accent hover:brightness-110 transition-all disabled:opacity-20"
              >
                <ArrowUp size={18} strokeWidth={2.5} className="text-cyber-bg" />
              </button>
            )}
          </div>
        </div>
        {/* Hidden file inputs */}
      </div>
    </div>
  );
}

// Sentinel id for the Claude Code "parasite" engine that lives in the model
// selector's extras slot. Picking it routes the turn through the wrapped
// Claude Code CLI instead of EchoBird's own agent_loop.
const PARASITE_CLAUDE_ID = 'claudecode';

// Mirrors `MAX_CONTEXT_BYTES` in echobird_core (services/agent_loop.rs).
// Used only for visualization — the actual trim happens server-side. Keep
// this value in sync with the Rust constant whenever it changes.
const MA_CONTEXT_MAX_BYTES = 300_000;

// Rough UTF-8 byte estimate of what gets serialized into the LLM payload.
// Doesn't have to match Rust's tally exactly — it just needs to grow at the
// right rate so the hint gives a useful usage estimate. Per message
// we add a constant envelope (~50–130 bytes) to cover role/content wrapper
// + tool_use_id/name overhead on the Rust side.
function estimateContextBytes(messages: readonly ChatMessage[]): number {
  const encoder = new TextEncoder();
  let total = 0;
  for (const m of messages) {
    switch (m.type) {
      case 'user':
      case 'assistant':
        total += 50 + encoder.encode(m.text).length;
        break;
      case 'tool_call':
        // Becomes two upstream messages: assistant tool_use + user tool_result
        total += 130 + encoder.encode(m.args).length + encoder.encode(m.output ?? '').length;
        break;
      // 'error' and 'cancelled' are frontend-only — never sent to the LLM
    }
  }
  return total;
}

// ===== Context usage =====
interface ContextUsageProps {
  contextBytes: number;
  contextSize?: number;
  editable: boolean;
  onContextSizeChange: (size: number) => void;
}

function ContextUsage({
  contextBytes,
  contextSize,
  editable,
  onContextSizeChange,
}: ContextUsageProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const canEdit = editable && contextSize !== undefined;
  const contextMaxBytes = contextSize ? contextSize * 4 : MA_CONTEXT_MAX_BYTES;
  const ratio = Math.max(0, Math.min(1, contextBytes / contextMaxBytes));
  const pct = Math.round(ratio * 100);
  const formatSize = (size: number) =>
    size >= 1_048_576 ? `${size / 1_048_576}M` : `${size / 1024}K`;
  const totalLabel = contextSize
    ? formatSize(contextSize)
    : `${Math.round(contextMaxBytes / 1024)}K`;
  const contextIndex = Math.max(
    0,
    CHAT_CONTEXT_OPTIONS.findIndex((size) => size === contextSize)
  );
  const contextProgress = `calc(9px + (100% - 18px) * ${contextIndex / (CHAT_CONTEXT_OPTIONS.length - 1)})`;
  const contextColorClass =
    ratio === 0
      ? 'text-cyber-text-secondary'
      : ratio <= 0.7
        ? 'text-green-500'
        : ratio <= 0.9
          ? 'text-cyber-warning'
          : 'text-cyber-error';
  const usageLine = t('mother.contextUsage')
    .replace('{used}', Math.round(contextBytes / 1024).toString())
    .replace('{total}', totalLabel)
    .replace('{pct}', pct.toString());
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!(event.target as HTMLElement).closest('[data-context-usage]')) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);
  return (
    <span
      data-context-usage
      className="group relative inline-flex h-8 shrink-0 items-center cursor-default"
    >
      <span
        onClick={() => canEdit && setOpen((value) => !value)}
        onKeyDown={(event) => {
          if (canEdit && (event.key === 'Enter' || event.key === ' ')) {
            event.preventDefault();
            setOpen((value) => !value);
          }
        }}
        role="progressbar"
        aria-label={usageLine}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        tabIndex={0}
        aria-haspopup={canEdit ? 'dialog' : undefined}
        aria-expanded={canEdit ? open : undefined}
        data-open={canEdit && open ? true : undefined}
        className="composer-tool-trigger inline-flex h-8 w-8 min-w-8 shrink-0 items-center justify-center rounded-button p-0 text-cyber-text-secondary focus-visible:outline focus-visible:outline-2 focus-visible:outline-cyber-accent"
      >
        <svg width="18" height="18" viewBox="0 0 20 20" fill="none" aria-hidden="true">
          <circle cx="10" cy="10" r="7" stroke="currentColor" strokeWidth="2" opacity="0.2" />
          <circle
            cx="10"
            cy="10"
            r="7"
            pathLength="100"
            stroke="currentColor"
            strokeWidth="2"
            strokeDasharray={`${ratio * 100} 100`}
            transform="rotate(-90 10 10)"
            className={contextColorClass}
          />
        </svg>
      </span>
      <span
        role="tooltip"
        className={`app-tooltip mother-context-popover absolute right-0 bottom-full z-[100] mb-2 rounded-md border px-2.5 py-1.5 text-xs leading-relaxed shadow-lg transition-opacity ${canEdit && open ? 'pointer-events-auto opacity-100' : 'pointer-events-none opacity-0 group-hover:opacity-100 group-focus-within:opacity-100'}`}
      >
        <span className="block">{usageLine}</span>
        {canEdit && open && (
          <div
            className="relative mt-1 h-8"
            style={{ '--context-progress': contextProgress } as React.CSSProperties}
          >
            <div aria-hidden="true" className="mother-reasoning-track">
              <div className="mother-context-fill" />
              {CHAT_CONTEXT_OPTIONS.map((size, position) => (
                <span
                  key={size}
                  className="mother-reasoning-stop"
                  style={{
                    left: `calc(9px + (100% - 18px) * ${position / (CHAT_CONTEXT_OPTIONS.length - 1)})`,
                  }}
                />
              ))}
            </div>
            <input
              type="range"
              min={0}
              max={CHAT_CONTEXT_OPTIONS.length - 1}
              step={1}
              value={contextIndex}
              aria-label={t('mother.contextSize')}
              aria-valuetext={formatSize(contextSize!)}
              onChange={(event) =>
                onContextSizeChange(
                  CHAT_CONTEXT_OPTIONS[Number(event.currentTarget.value)] ?? CHAT_CONTEXT_OPTIONS[0]
                )
              }
              className="mother-reasoning-slider mother-context-slider relative block w-full"
            />
          </div>
        )}
        {!canEdit && (
          <span className="mt-1 block text-white/60">{t('mother.contextSizeLocked')}</span>
        )}
      </span>
    </span>
  );
}
