import { Plus, Trash2 } from 'lucide-react';
import { useI18n } from '../../hooks/useI18n';
import { useUsageClock } from '../../hooks/useUsageClock';
import { useNavigationStore } from '../../stores/navigationStore';
import { useMotherAgent } from './context';

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
        className="w-full h-12 px-3 text-[14px] font-semibold border border-cyber-border rounded-card transition-colors flex items-center justify-center gap-2 text-cyber-text-secondary hover:text-cyber-text hover:bg-cyber-elevated disabled:opacity-40"
      >
        <Plus size={14} />
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
              className={`relative w-full h-12 shrink-0 px-[11px] py-1 pr-9 grid grid-cols-[minmax(0,1fr)] auto-rows-[16px] items-center rounded-card border-2 text-left transition-colors ${active ? 'border-cyber-accent/70 bg-cyber-accent/10' : 'border-transparent bg-cyber-surface/60 hover:bg-cyber-elevated/70'} ${isProcessing ? 'opacity-40' : ''}`}
            >
              <button
                type="button"
                onClick={() => !isProcessing && selectConversation(conversation.id)}
                disabled={isProcessing}
                aria-current={active ? 'true' : undefined}
                aria-label={conversation.title || t('mother.newConversation')}
                className="absolute inset-0 rounded-card focus-visible:outline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-cyber-accent"
              />
              <div className="relative pointer-events-none text-sm leading-[16px] font-semibold text-cyber-text truncate">
                {conversation.title || t('mother.newConversation')}
              </div>
              <div className="relative pointer-events-none leading-[16px] text-xs text-cyber-text-secondary truncate">
                {t('mother.conversationMessageCount').replace('{count}', String(messageCount))}
                {' · '}
                {formatUpdatedTime(conversation.updatedAt)}
              </div>
              <button
                type="button"
                aria-label={`${t('btn.delete')} ${conversation.title || t('mother.newConversation')}`}
                disabled={isProcessing}
                onClick={(event) => {
                  event.stopPropagation();
                  if (!isProcessing) void deleteConversation(conversation.id);
                }}
                className="absolute right-3 top-1/2 -translate-y-1/2 p-0.5 text-cyber-text-secondary hover:text-red-500 transition-colors"
              >
                <Trash2 size={20} />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
