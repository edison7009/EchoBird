import React, { useState, useEffect, useLayoutEffect, useRef, useCallback } from 'react';
import { ArrowUp, ChevronDown, Square } from 'lucide-react';
import { RemoteModelSelector, type ModelOption } from '../../components/RemoteModelSelector';
import { getModelIcon } from '../../components/cards/ModelCard';
import { PendingChipsRow } from '../../components/PendingChipsRow';
import { ChatBubble, ToolCallCard } from '../../components/chat';
import { buildPendingMessage } from '../../utils/buildPendingMessage';
import { useI18n } from '../../hooks/useI18n';
import { useToast } from '../../components/Toast';
import * as api from '../../api/tauri';
import { useNavigationStore } from '../../stores/navigationStore';
import { useMotherAgent } from './context';
import { MA_PAGE_SIZE, type ChatMessage } from './types';

// ===== Main Content (center area) — CHAT =====
export function MotherAgentMain() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const {
    models,
    agentModel,
    setAgentModel,
    chatInput,
    setChatInput,
    chatOutput,
    isProcessing,
    agentModelData: _agentModelData,
    agentState: _agentState,
    chatEndRef,
    chatInputRef,
    handleChatSend,
    sendMessage,

    sshServers: _sshServers,
    selectedServerId,
    clearChat,
    abortAgent,
    maDiskTotal,
    loadOlderChat,
    parasiteAgent,
    setParasiteAgent,
    parasiteAvailable,
  } = useMotherAgent();

  // Build model list for RemoteModelSelector (with icons)
  const modelList: ModelOption[] = React.useMemo(
    () =>
      models.map((m) => ({
        id: m.internalId,
        name: m.name,
        icon: getModelIcon(m.name, m.modelId),
      })),
    [models]
  );

  // Estimated UTF-8 byte footprint of the current chat — drives the ring
  // around the "?" hint so users can see how close they are to the
  // agent_loop trim threshold (MAX_CONTEXT_BYTES in echobird_core).
  // estimateContextBytes re-encodes the whole transcript; recomputing it on
  // every streamed token is O(n^2) and only drives a coarse visual ring, so
  // debounce it (recompute ~300ms after updates settle) instead of per-token.
  const [contextBytes, setContextBytes] = useState(0);
  useEffect(() => {
    const id = setTimeout(() => setContextBytes(estimateContextBytes(chatOutput)), 300);
    return () => clearTimeout(id);
  }, [chatOutput]);

  // Listen for clear-chat event from title bar
  useEffect(() => {
    const handler = () => clearChat();
    window.addEventListener('clear-chat', handler);
    return () => window.removeEventListener('clear-chat', handler);
  }, [clearChat]);

  const [_serverModel, setServerModel] = useState<string | null>(null);
  const _fileInputRef = useRef<HTMLInputElement>(null!);
  const _imageInputRef = useRef<HTMLInputElement>(null!);

  const [pendingFiles, setPendingFiles] = useState<
    Array<{ id: string; name: string; type: 'file' | 'image'; preview?: string }>
  >([]);

  // Wrap handleChatSend to append pending file info as text
  const localSend = useCallback(() => {
    const hasFiles = pendingFiles.length > 0;

    if (hasFiles) {
      const { messageText, chips } = buildPendingMessage(chatInput, pendingFiles, [], []);

      setPendingFiles([]);
      setChatInput('');
      sendMessage(messageText, chatInput.trim(), chips);
    } else {
      handleChatSend();
    }
  }, [pendingFiles, chatInput, setChatInput, handleChatSend, sendMessage, setPendingFiles]);

  // File handling
  const _handleFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files) return;
    Array.from(files).forEach((f) => {
      const id = `file-${Date.now()}-${f.name}`;
      setPendingFiles((prev) => [...prev, { id, name: f.name, type: 'file' }]);
    });
    e.target.value = '';
  };

  const _handleImageSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files) return;
    Array.from(files).forEach((f) => {
      const id = `img-${Date.now()}-${f.name}`;
      const reader = new FileReader();
      reader.onload = () => {
        setPendingFiles((prev) => [
          ...prev,
          { id, name: f.name, type: 'image', preview: reader.result as string },
        ]);
      };
      reader.readAsDataURL(f);
    });
    e.target.value = '';
  };

  // Poll Local Server status
  useEffect(() => {
    const check = async () => {
      try {
        const info = await api.getLlmServerInfo();
        setServerModel(info.running ? info.modelName || 'unknown' : null);
      } catch {
        setServerModel(null);
      }
    };
    check();
    const interval = setInterval(check, 3000);
    return () => clearInterval(interval);
  }, []);

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

  // Reset pagination + re-arm sticky when the server changes
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setDisplayCount(PAGE_SIZE);
    stickToBottomRef.current = true;
    requestAnimationFrame(snapToBottom);
  }, [selectedServerId, snapToBottom]);

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

  const handleScroll = () => {
    const container = chatContainerRef.current;
    if (!container) return;
    const distanceFromBottom =
      container.scrollHeight - container.scrollTop - container.clientHeight;
    const isNearBottom = distanceFromBottom < 80;
    // Sticky tracks the actual scroll position. Programmatic snaps land
    // at distance≈0, so they correctly keep sticky=true; user wheel/drag
    // away from the bottom flips it off.
    stickToBottomRef.current = isNearBottom;
    setShowScrollBtn(!isNearBottom && chatOutput.length > 0);

    if (container.scrollTop !== 0) return;

    // Phase 1: more in-memory messages to show
    if (displayCount < chatOutput.length) {
      setShowSkeleton(true);
      const prevScrollHeight = container.scrollHeight;
      setTimeout(() => {
        setShowSkeleton(false);
        setDisplayCount((c) => Math.min(c + PAGE_SIZE, chatOutput.length));
        requestAnimationFrame(() => {
          if (chatContainerRef.current) {
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
    loadOlderChat()
      .then((older) => {
        setShowSkeleton(false);
        if (older.length === 0) return;
        setDisplayCount((c) => c + older.length);
        requestAnimationFrame(() => {
          if (chatContainerRef.current) {
            chatContainerRef.current.scrollTop =
              chatContainerRef.current.scrollHeight - prevScrollHeight2;
          }
        });
      })
      .catch(() => {
        setShowSkeleton(false);
      });
  };

  const scrollToBottom = useCallback(() => {
    stickToBottomRef.current = true;
    setShowScrollBtn(false);
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [chatEndRef]);

  return (
    <div className="flex flex-col h-full">
      {/* Chat conversation area */}
      <div className="relative flex-1">
        <div
          ref={chatContainerRef}
          onScroll={handleScroll}
          className="absolute inset-0 overflow-y-auto slim-scroll p-4"
        >
          {/* Chat messages — markdown stream */}
          <div className="pt-2 pb-2">
            {/* Skeleton placeholders — shown briefly when lazy-loading older messages */}
            {showSkeleton &&
              [0, 1, 2].map((i) => (
                <ChatBubble key={`sk-${i}`} role="skeleton" content="" variant="mother" />
              ))}
            {chatOutput.slice(-displayCount).map((msg, i, arr) => {
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
                const lastOutput = chatOutput[chatOutput.length - 1];
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
              (chatOutput.length === 0 ||
                chatOutput[chatOutput.length - 1]?.type !== 'assistant') && (
                <ChatBubble role="assistant" content="" variant="mother" isStreaming={true} />
              )}
            <div ref={chatEndRef} />
          </div>
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
        <div className="bg-cyber-elevated rounded-2xl p-2.5 border border-cyber-border">
          {/* Pending attachments chips — shared component */}
          <PendingChipsRow
            files={pendingFiles}
            onRemoveFile={(id) => setPendingFiles((prev) => prev.filter((x) => x.id !== id))}
          />
          <textarea
            ref={chatInputRef}
            value={chatInput}
            onChange={(e) => setChatInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                if (!isProcessing) localSend();
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
                const currentId = parasiteAgent || agentModel;
                if (currentId && id && id !== currentId) {
                  showToast(
                    'success',
                    t('mother.switchEngineHintReason'),
                    t('mother.switchEngineHint')
                  );
                }
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
                  // claude-opus-4-* models) so the parasite engine row is
                  // visually distinct from regular Claude models.
                  icon: '/icons/tools/claude.svg',
                  disabled: !parasiteAvailable.includes(PARASITE_CLAUDE_ID),
                  disabledLabel: t('status.notInstalled'),
                },
              ]}
            />
            {isProcessing ? (
              <button
                onClick={() => abortAgent()}
                className="w-8 h-8 rounded-lg flex items-center justify-center bg-red-500/20 hover:bg-red-500/30 transition-colors"
              >
                <Square size={14} fill="#f87171" className="text-red-400" />
              </button>
            ) : (
              <button
                onClick={localSend}
                disabled={!chatInput.trim()}
                className="w-8 h-8 rounded-lg flex items-center justify-center bg-cyber-accent hover:brightness-110 transition-all disabled:opacity-20"
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
// right rate so the ring is a useful "how full am I" indicator. Per message
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

// ===== Context Usage Ring =====
// Concentric SVG ring rendered around the "?" glyph. Fills as the estimated
// payload approaches MA_CONTEXT_MAX_BYTES; transitions to amber then red to
// telegraph "older messages will start dropping soon".

interface ContextRingProps {
  ratio: number;
  children: React.ReactNode;
}

function ContextRing({ ratio, children }: ContextRingProps) {
  const clamped = Math.min(1, Math.max(0, ratio));
  const radius = 9;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - clamped);

  // Stop colors match the industry-standard context-usage palette
  // (Cursor / ChatGPT / Claude): cyan accent → amber → red as the
  // payload fills toward the trim threshold.
  const strokeClass =
    clamped < 0.7
      ? 'stroke-cyber-accent'
      : clamped < 0.9
        ? 'stroke-cyber-warning'
        : 'stroke-cyber-error';

  return (
    <span className="relative inline-flex h-5 w-5 items-center justify-center">
      <svg aria-hidden="true" viewBox="0 0 20 20" className="absolute inset-0 -rotate-90">
        <circle
          cx="10"
          cy="10"
          r={radius}
          fill="none"
          strokeWidth="1.5"
          className="stroke-cyber-border/40"
        />
        <circle
          cx="10"
          cy="10"
          r={radius}
          fill="none"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          className={`${strokeClass} transition-all duration-300`}
        />
      </svg>
      <span className="relative">{children}</span>
    </span>
  );
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
    <span className="group relative inline-flex items-center">
      <span
        aria-label={ariaLabel}
        className="inline-flex h-5 w-5 items-center justify-center rounded-full bg-cyber-elevated font-sans text-xs font-medium leading-none text-cyber-text-secondary cursor-help select-none hover:bg-cyber-accent/15 hover:text-cyber-accent transition-colors"
      >
        <ContextRing ratio={ratio}>?</ContextRing>
      </span>
      <span
        role="tooltip"
        className="pointer-events-none absolute right-0 bottom-full z-[100] mb-2 w-64 rounded border border-cyber-accent/40 bg-cyber-elevated px-3 py-2 text-[11px] leading-relaxed text-cyber-text shadow-cyber-card backdrop-blur-sm opacity-0 group-hover:opacity-100 transition-opacity"
      >
        {/* Caret — rotated square poking down out of the tooltip's bottom edge,
            aligned roughly above the ? glyph at the right side (tooltip
            extends leftward into the textarea's empty space, since right
            of the ? is where the model selector + send button live). */}
        <span
          aria-hidden="true"
          className="absolute -bottom-1 right-2 h-2 w-2 rotate-45 border-b border-r border-cyber-accent/40 bg-cyber-elevated"
        />
        <span className="block">{baseTip}</span>
        <span className="mt-1 block border-t border-cyber-border/40 pt-1 text-cyber-text-secondary">
          {usageLine}
        </span>
      </span>
    </span>
  );
}
