import { isSmartRouterId } from '../../utils/smartRouter';
import React, { useState, useEffect, useLayoutEffect, useRef, useCallback } from 'react';
import { ArrowUp, ChevronDown, Square } from 'lucide-react';
import { RemoteModelSelector, type ModelOption } from '../../components/RemoteModelSelector';
import { getModelIcon } from '../../components/cards/ModelCard';
import { ChatBubble, ToolCallCard } from '../../components/chat';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useMotherAgent } from './context';
import { QuickCommandChips } from './QuickCommandChips';
import { ReasoningControl } from './ReasoningControl';
import { MA_PAGE_SIZE, type ChatMessage } from './types';

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
  } = useMotherAgent();
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

  // Build model list for RemoteModelSelector (with icons)
  const modelList: ModelOption[] = React.useMemo(
    () =>
      models.map((m) => ({
        id: m.internalId,
        name: m.name,
        icon: getModelIcon('', m.modelId),
        iconKind: isSmartRouterId(m.internalId) ? ('router' as const) : undefined,
      })),
    [models]
  );

  // Estimated UTF-8 byte footprint of the current chat — shown in the
  // "?" hint so users can see how close they are to the
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
  const PAGE_SIZE = MA_PAGE_SIZE;
  const [displayCount, setDisplayCount] = useState(PAGE_SIZE);
  const [showSkeleton, setShowSkeleton] = useState(false);
  const paginationGenerationRef = useRef(0);

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

  // Reset pagination + re-arm sticky when the conversation changes
  useEffect(() => {
    paginationGenerationRef.current += 1;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setDisplayCount(PAGE_SIZE);
    setShowSkeleton(false);
    stickToBottomRef.current = true;
    requestAnimationFrame(snapToBottom);
  }, [selectedServerId, activeConversationId, snapToBottom, PAGE_SIZE]);

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
  }, [chatOutput, displayCount, isProcessing, snapToBottom]);

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

    if (container.scrollTop !== 0) return;
    const generation = paginationGenerationRef.current;

    // Phase 1: more in-memory messages to show
    if (displayCount < visibleChatOutput.length) {
      setShowSkeleton(true);
      const prevScrollHeight = container.scrollHeight;
      setTimeout(() => {
        if (generation !== paginationGenerationRef.current) return;
        setShowSkeleton(false);
        setDisplayCount((c) => Math.min(c + PAGE_SIZE, visibleChatOutput.length));
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
      setDisplayCount((c) => c + visibleLoaded);
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

  return (
    <div className="flex flex-col h-full">
      {/* Chat conversation area */}
      <div className="relative flex-1">
        <div
          ref={chatContainerRef}
          onScroll={handleScroll}
          className={`absolute inset-0 overflow-y-auto slim-scroll ${showWelcome ? 'py-4' : 'p-4'}`}
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
            /* Chat messages — markdown stream */
            <div className="pt-2 pb-2">
              {/* Skeleton placeholders — shown briefly when lazy-loading older messages */}
              {showSkeleton &&
                [0, 1, 2].map((i) => (
                  <ChatBubble key={`sk-${i}`} role="skeleton" content="" variant="mother" />
                ))}
              {visibleChatOutput.slice(-displayCount).map((msg, i, arr) => {
                if (msg.type === 'user') {
                  return (
                    <ChatBubble
                      key={i}
                      role="user"
                      content={msg.text}
                      variant="mother"
                      chips={msg.chips}
                    />
                  );
                }
                if (msg.type === 'tool_call') {
                  return (
                    <ToolCallCard
                      key={`${i}-${msg.id}`}
                      name={msg.name}
                      args={msg.args}
                      status={msg.status}
                      output={msg.output}
                    />
                  );
                }
                if (msg.type === 'assistant') {
                  const isLast = arr.slice(i + 1).every((m) => m.type !== 'assistant');
                  const lastOutput = visibleChatOutput[visibleChatOutput.length - 1];
                  const isCurrentResponse = isLast && lastOutput?.type === 'assistant';
                  return (
                    <ChatBubble
                      key={i}
                      role="assistant"
                      content={msg.text}
                      variant="mother"
                      isStreaming={isProcessing && isCurrentResponse}
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
                  return <ChatBubble key={i} role="error" content={text} variant="mother" />;
                }
                return null;
              })}
              {/* Typing indicator — show when processing and no new assistant response has started */}
              {isProcessing &&
                (visibleChatOutput.length === 0 ||
                  visibleChatOutput[visibleChatOutput.length - 1]?.type !== 'assistant') && (
                  <ChatBubble role="assistant" content="" variant="mother" isStreaming={true} />
                )}
              <div ref={chatEndRef} />
            </div>
          )}
        </div>
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
        <div className="bg-cyber-elevated/75 rounded-2xl p-2.5">
          <textarea
            ref={chatInputRef}
            value={chatInput}
            onChange={(e) => {
              if (chatPrefillRef) chatPrefillRef.current = null;
              setChatInput(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                if (!isProcessing) handleChatSend();
              }
            }}
            placeholder={t('mother.enterMessage')}
            disabled={isProcessing}
            rows={2}
            className="w-full bg-transparent px-2 py-1 text-sm text-cyber-text font-sans font-medium outline-none placeholder:text-cyber-text-muted disabled:opacity-30 resize-none"
          />
          <div className="flex items-center justify-end gap-1.5">
            {/* Soft nudge sits LEFT of the model selector so it doesn't
                interrupt the natural "pick model → send" action flow on the
                right. Hidden once the user is already in parasite mode. */}
            {parasiteAgent !== PARASITE_CLAUDE_ID && (
              <ParasiteHint
                ccInstalled={parasiteAvailable.includes(PARASITE_CLAUDE_ID)}
                contextBytes={contextBytes}
                contextMaxBytes={MA_CONTEXT_MAX_BYTES}
              />
            )}
            <RemoteModelSelector
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

// ===== Parasite Hint =====
// Themed "?" glyph next to the model selector that nudges the user toward
// Claude Code when they're chatting with EchoBird's own short-memory loop.
// Tooltip copy adapts to whether CC is already installed (action: switch)
// vs not installed (action: install first). Matches the visual pattern of
// AppManager's relay-mode "?" — bottom-anchored caret since we live in the
// bottom toolbar.

interface ParasiteHintProps {
  ccInstalled: boolean;
  contextBytes: number;
  contextMaxBytes: number;
}

function ParasiteHint({ ccInstalled, contextBytes, contextMaxBytes }: ParasiteHintProps) {
  const { t } = useI18n();
  const baseTip = ccInstalled
    ? t('mother.parasiteTipInstalled')
    : t('mother.parasiteTipNotInstalled');
  const ratio = contextBytes / contextMaxBytes;
  const pct = Math.min(100, Math.round(ratio * 100));
  const usageLine = t('mother.contextUsage')
    .replace('{used}', Math.round(contextBytes / 1024).toString())
    .replace('{total}', Math.round(contextMaxBytes / 1024).toString())
    .replace('{pct}', pct.toString());
  const ariaLabel = `${baseTip} ${usageLine}`;

  return (
    <span className="group relative inline-flex h-8 shrink-0 items-center cursor-default">
      <span
        aria-label={ariaLabel}
        className="inline-flex h-5 w-5 items-center justify-center rounded-full bg-cyber-elevated font-sans text-xs font-medium leading-none text-cyber-text-secondary select-none hover:bg-cyber-accent/15 hover:text-cyber-accent transition-colors"
      >
        ?
      </span>
      <span
        role="tooltip"
        className="app-tooltip pointer-events-none absolute right-0 bottom-full z-[100] mb-2 w-max max-w-64 rounded-md border px-2.5 py-1.5 text-xs leading-relaxed shadow-lg opacity-0 group-hover:opacity-100 transition-opacity"
      >
        <span className="block">{baseTip}</span>
        <span className="mt-1 block border-t border-white/15 pt-1 text-white/75">{usageLine}</span>
      </span>
    </span>
  );
}
