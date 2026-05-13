import React, { useState, useEffect, useLayoutEffect, useRef, useCallback } from 'react';
import { ArrowUp, ChevronDown, Square } from 'lucide-react';
import { RemoteModelSelector, type ModelOption } from '../../components/RemoteModelSelector';
import { getModelIcon } from '../../components/cards/ModelCard';
import { PendingChipsRow } from '../../components/PendingChipsRow';
import { ChatBubble, ToolCallCard } from '../../components/chat';
import { buildPendingMessage } from '../../utils/buildPendingMessage';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import { useNavigationStore } from '../../stores/navigationStore';
import { useMotherAgent } from './context';
import { MA_PAGE_SIZE } from './types';

// ===== Main Content (center area) — CHAT =====
export function MotherAgentMain() {
  const { t, locale: _locale } = useI18n();
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
    handleChatSend,
    sendMessage,

    sshServers: _sshServers,
    selectedServerId,
    clearChat,
    abortAgent,
    maDiskTotal,
    loadOlderChat,
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

  // Listen for clear-chat event from title bar
  useEffect(() => {
    const handler = () => clearChat();
    window.addEventListener('clear-chat', handler);
    return () => window.removeEventListener('clear-chat', handler);
  }, [clearChat]);

  const [_publicIP, setPublicIP] = useState('...');
  const [remoteHints, setRemoteHints] = useState<Array<{ action: string; agent?: string }>>([]);
  const [_serverModel, setServerModel] = useState<string | null>(null);
  const chatInputRef = useRef<HTMLTextAreaElement>(null!);
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

  useEffect(() => {
    fetch('https://api.ipify.org?format=text')
      .then((r) => r.text())
      .then((ip) => setPublicIP(ip))
      .catch(() => setPublicIP('offline'));
    // Load quick-hint buttons from bundled assets (offline-first).
    api
      .getMotherHints()
      .then((s) => {
        try {
          const data = JSON.parse(s);
          setRemoteHints(data.hints || []);
        } catch {
          setRemoteHints([]);
        }
      })
      .catch(() => setRemoteHints([]));
  }, []);

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
          {/* Quick prompt hints — scrolls with content */}
          <div className="mb-2 select-none">
            {remoteHints.length > 0 && (
              <div className="flex flex-wrap gap-2 py-2">
                {remoteHints.map((hint, i) => {
                  // showSpecs swaps to a local-machine wording when 127.0.0.1
                  // is selected — otherwise the agent often refuses, treating
                  // "server" prompts as a remote/privileged operation.
                  const isLocalShowSpecs =
                    hint.action === 'showSpecs' && selectedServerId === 'local';
                  const i18nKey = (
                    isLocalShowSpecs
                      ? 'mother.hintShowSpecsLocal'
                      : `mother.hint${hint.action[0].toUpperCase()}${hint.action.slice(1)}`
                  ) as any;
                  const label = t(i18nKey).replace('{agent}', hint.agent || '');
                  // Skip hints whose i18n key was removed (label equals raw key)
                  if (label === i18nKey) return null;
                  return (
                    <button
                      key={i}
                      onClick={() => {
                        // Replace input contents — don't append. Each hint
                        // is a self-contained prompt; concatenating them
                        // produces nonsense for the agent.
                        setChatInput(label);
                        const el = chatInputRef.current;
                        if (el) {
                          el.focus();
                          requestAnimationFrame(() => {
                            el.selectionStart = el.selectionEnd = label.length;
                          });
                        }
                      }}
                      className="px-3 py-1.5 text-xs rounded-full bg-cyber-surface border border-cyber-border text-cyber-text-secondary hover:bg-cyber-elevated hover:text-cyber-text hover:border-cyber-text-muted/50 transition-colors cursor-pointer"
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            )}
          </div>

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
            <RemoteModelSelector
              models={modelList}
              currentModelId={agentModel}
              loading={false}
              onSelect={(id) => setAgentModel(id || null)}
              placeholder={t('mother.selectModel')}
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
