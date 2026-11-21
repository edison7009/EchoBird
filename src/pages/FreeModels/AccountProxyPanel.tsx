import { accountProxySources, useAccountProviderFilter } from '../../hooks/useAccountCapability';
import { useEffect, useState, type CSSProperties } from 'react';
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
import { useBrandColour } from '../../utils/brandColour';
import '../../components/brandCards.css';

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
  const configurationActive = useNavigationStore(
    (state) => state.activePage === 'freeModels' && state.smartAccessTab === 'configuration'
  );
  const [accountCounts, setAccountCounts] = useState<Partial<Record<AccountProvider, number>>>({});

  useEffect(() => {
    if (voice || !configurationActive) return;
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
  }, [configurationActive, voice, available]);

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
    <div className={voice ? 'brand-deck' : 'brand-deck flex-1 px-2 pt-1 pb-2 overflow-y-auto'}>
      {sortedProviders.map((provider) => (
        <AccountProxyCard
          key={provider.id}
          iconSrc={accountProviderIconSrc(provider.icon)}
          name={provider.name}
          disabled={disabled}
          label={`${t(voice ? 'voice.accountInput' : 'freeModels.addToRouter')}: ${provider.name}`}
          onClick={() => {
            if (!disabled)
              openAccountModelForm(provider.id, context, voice ? 'voiceInput' : 'freeRouter');
          }}
          count={
            counts[provider.id] === undefined
              ? '—'
              : t(
                  counts[provider.id] === 1
                    ? 'accountProxy.accountCountOne'
                    : 'accountProxy.accountCountOther'
                ).replace('{count}', String(counts[provider.id]))
          }
          empty={!counts[provider.id]}
        />
      ))}
    </div>
  );
}

/** One provider as a deck card; the whole card opens the account form. */
function AccountProxyCard({
  iconSrc,
  name,
  label,
  count,
  empty,
  disabled,
  onClick,
}: {
  iconSrc: string;
  name: string;
  label: string;
  count: string;
  empty: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  const colour = useBrandColour(iconSrc);
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      aria-label={label}
      className="brand-deck-card w-full text-left"
      style={{ '--deck-from': colour.from, '--deck-to': colour.to } as CSSProperties}
    >
      <img
        src={iconSrc}
        alt=""
        draggable={false}
        className={`brand-deck-icon${colour.glyph ? ' is-glyph' : ''}`}
      />
      <span className="brand-deck-name">{name}</span>
      <span className={`brand-deck-note tabular-nums${empty ? ' is-empty' : ''}`}>{count}</span>
    </button>
  );
}
