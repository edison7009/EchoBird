import { accountProxySources, useAccountProviderFilter } from '../../hooks/useAccountPlugin';
import { useEffect, useState } from 'react';
import {
  accountProviderIconSrc,
  accountSpeechProviders,
  listProxyAccounts,
  type ProxyAccount,
} from '../../api/accountProxy';
import type { AccountProvider } from '../../api/types';
import { useI18n } from '../../hooks/useI18n';
import { useNavigationStore } from '../../stores/navigationStore';
import { useModelNexus } from '../ModelNexus/context';

export function openAccountModelForm(
  provider: AccountProvider,
  context: ReturnType<typeof useModelNexus>,
  destination: 'freeRouter' | 'voiceInput' = 'freeRouter'
) {
  context.setNewModelForm({
    name:
      (destination === 'voiceInput' ? accountSpeechProviders : accountProxySources()).find(
        (p) => p.id === provider
      )?.name ?? '',
    baseUrl: '',
    anthropicUrl: '',
    apiKey: '',
    modelId: '',
    accountSource: { provider, accountId: '' },
  });
  context.setEditingModelId(null);
  context.setKeyDestroyed(false);
  context.setShowApiKey(false);
  context.setModelModalDestination(destination);
  context.setShowAddModelModal(true);
}

export function AccountProxyPanel({
  voice = false,
  disabled = false,
  accounts,
  accountCounts: suppliedCounts,
}: {
  voice?: boolean;
  disabled?: boolean;
  accounts?: ProxyAccount[];
  accountCounts?: Partial<Record<AccountProvider, number>>;
}) {
  const context = useModelNexus();
  const available = useAccountProviderFilter();
  const { t } = useI18n();
  const activePage = useNavigationStore((state) => state.activePage);
  const [accountCounts, setAccountCounts] = useState<Partial<Record<AccountProvider, number>>>({});

  useEffect(() => {
    if (voice || activePage !== 'freeModels') return;
    let ignore = false;
    void Promise.resolve().then(async () => {
      if (ignore) return;
      const availableProviders = accountProxySources().filter(available);
      const results = await Promise.allSettled(
        availableProviders.map((provider) => listProxyAccounts(provider.id))
      );
      if (ignore) return;
      setAccountCounts((previous) => {
        const counts = { ...previous };
        results.forEach((result, index) => {
          if (result.status === 'fulfilled')
            counts[availableProviders[index].id] = result.value.length;
        });
        return counts;
      });
    });
    return () => {
      ignore = true;
    };
  }, [activePage, voice, available]);

  const providers = (voice ? accountSpeechProviders : accountProxySources()).filter(available);
  const counts: Partial<Record<AccountProvider, number>> = voice
    ? (suppliedCounts ?? {})
    : accountCounts;
  if (voice && suppliedCounts === undefined && accounts !== undefined)
    providers.forEach((provider) => {
      counts[provider.id] = accounts.filter(
        (account) => account.source.provider === provider.id
      ).length;
    });
  const sortedProviders = [...providers].sort((a, b) => (counts[b.id] ?? 0) - (counts[a.id] ?? 0));

  return (
    <div className={voice ? 'space-y-2' : 'flex-1 px-2 pt-0.5 pb-2 overflow-y-auto space-y-2'}>
      {sortedProviders.map((provider) => (
        <button
          key={provider.id}
          type="button"
          disabled={disabled}
          onClick={() => {
            if (!disabled)
              openAccountModelForm(provider.id, context, voice ? 'voiceInput' : 'freeRouter');
          }}
          aria-label={`${t(voice ? 'voice.accountInput' : 'freeModels.addToRouter')}: ${provider.name}`}
          className="w-full h-14 shrink-0 flex items-center gap-3 px-[11px] border-2 border-transparent bg-cyber-surface/60 rounded-card text-left enabled:hover:bg-cyber-elevated/70 disabled:opacity-40 disabled:grayscale transition-colors"
        >
          <img
            src={accountProviderIconSrc(provider.icon)}
            alt=""
            className="w-8 h-8 shrink-0 object-contain"
          />
          <span className="flex-1 min-w-0 text-base font-bold text-cyber-text truncate">
            {provider.name}
          </span>
          <span className="shrink-0 text-xs text-cyber-text-secondary tabular-nums whitespace-nowrap">
            {counts[provider.id] === undefined
              ? '—'
              : t(
                  counts[provider.id] === 1
                    ? 'accountProxy.accountCountOne'
                    : 'accountProxy.accountCountOther'
                ).replace('{count}', String(counts[provider.id]))}
          </span>
        </button>
      ))}
    </div>
  );
}
