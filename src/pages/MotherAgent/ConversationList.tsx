import type { CSSProperties } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useUsageClock } from '../../hooks/useUsageClock';
import { useNavigationStore } from '../../stores/navigationStore';
import { useMotherAgent } from './context';
import '../../components/brandCards.css';

const accent = { '--brand': 'rgb(var(--accent-rgb))' } as CSSProperties;

export function ConversationList() {
  const { t, locale } = useI18n();
  const {
    conversations,
    activeConversationId,
    createConversation,
    selectConversation,
    deleteConversation,
    isProcessing,
  } = useMotherAgent();
  const isMotherActive = useNavigationStore((s) => s.activePage === 'mother');
  const now = useUsageClock(isMotherActive);

  const relativeTime = new Intl.RelativeTimeFormat(locale, { numeric: 'always' });
  const formatUpdatedTime = (updatedAt: number) => {
    const minutes = Math.floor(Math.max(0, now - updatedAt) / 60_000);
    if (minutes < 1) return t('mother.justNow');
    if (minutes < 60) return relativeTime.format(-minutes, 'minute');
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return relativeTime.format(-hours, 'hour');
    const days = Math.floor(hours / 24);
    if (days < 30) return relativeTime.format(-days, 'day');
    const months = Math.floor(days / 30);
    if (months < 12) return relativeTime.format(-months, 'month');
    return relativeTime.format(-Math.floor(months / 12), 'year');
  };

  return (
    <div>
      <button
        type="button"
        onClick={() => !isProcessing && createConversation()}
        disabled={isProcessing}
        className="brand-new-row"
      >
        <span className="brand-deck-plus" aria-hidden="true">
          <Plus size={14} strokeWidth={3} />
        </span>
        {t('mother.newConversation')}
      </button>
      <div className="mt-2 space-y-2">
        {conversations.map((conversation) => {
          const active = conversation.id === activeConversationId;
          const messageCount =
            conversation.messageCount ??
            conversation.messages.filter(
              (message) => message.type === 'user' || message.type === 'assistant'
            ).length;
          return (
            <div
              key={conversation.id}
              style={accent}
              className={`brand-row is-quiet relative flex w-full h-14 shrink-0 items-center gap-3 px-[11px] py-1 pr-10 rounded-card border-2 text-left ${active ? 'is-selected' : 'border-transparent'} ${isProcessing ? 'opacity-40' : ''}`}
            >
              <button
                type="button"
                onClick={() => !isProcessing && selectConversation(conversation.id)}
                disabled={isProcessing}
                aria-current={active ? 'true' : undefined}
                aria-label={conversation.title || t('mother.newConversation')}
                className="absolute inset-0 rounded-card focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-cyber-accent"
              />
              <div className="relative z-10 min-w-0 flex-1 pointer-events-none grid grid-rows-[16px_16px] items-center">
                <div className="truncate text-sm font-bold leading-4 text-cyber-text">
                  {conversation.title || t('mother.newConversation')}
                </div>
                <div className="truncate text-[10px] leading-4 text-cyber-text-secondary">
                  {t('mother.conversationMessageCount').replace('{count}', String(messageCount))}
                  {' · '}
                  {formatUpdatedTime(conversation.updatedAt)}
                </div>
              </div>
              <button
                type="button"
                aria-label={`${t('btn.delete')} ${conversation.title || t('mother.newConversation')}`}
                disabled={isProcessing}
                onClick={(event) => {
                  event.stopPropagation();
                  if (!isProcessing) void deleteConversation(conversation.id);
                }}
                className="absolute right-3 top-1/2 z-10 flex h-5 w-5 -translate-y-1/2 items-center justify-center p-0 text-cyber-text-secondary transition-colors hover:text-red-500"
              >
                <Trash2 size={16} />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
