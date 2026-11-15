import { accountProxySources, useAccountProviderFilter } from '../../hooks/useAccountCapability';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  accountSpeechProviders,
  accountModelPickerOptions,
  cacheAccountProxyModels,
  getAccountProxyModels,
  getAccountSpeechModels,
  fixedAccountSpeechModels,
  getCachedAccountProxyModels,
  listProxyAccounts,
  type ProxyAccount,
} from '../../api/accountProxy';
import type { AccountProvider, AccountSource } from '../../api/types';
import { ModelIdCombobox } from '../../components/ModelIdCombobox';
import { useI18n } from '../../hooks/useI18n';
import { useToast } from '../../components/Toast';
import { accountProxyIdentityLabel, accountProxyPlanLabel } from '../AppManager/accountPlanLabel';
import type { NewModelForm } from './context';

export function AccountModelFields({
  source,
  modelId,
  modelIdOptions,
  setForm,
  active = true,
  voice = false,
}: {
  source: AccountSource;
  modelId: string;
  modelIdOptions?: string[];
  setForm: React.Dispatch<React.SetStateAction<NewModelForm>>;
  active?: boolean;
  voice?: boolean;
}) {
  const { t } = useI18n();
  const available = useAccountProviderFilter();
  active = active && available({ id: source.provider });
  const { showToast } = useToast();
  const { provider, accountId } = source;
  const [accounts, setAccounts] = useState<ProxyAccount[]>([]);
  const accountsRef = useRef<ProxyAccount[]>([]);
  const [loadedProvider, setLoadedProvider] = useState<AccountProvider | null>(null);
  const loading = loadedProvider !== provider;
  const waitingForLabels =
    (provider === 'qoder' ||
      provider === 'qoderCn' ||
      provider === 'kiro' ||
      provider === 'doubaowork' ||
      provider === 'dola') &&
    loading;
  const [fetching, setFetching] = useState(false);
  const fetchingRef = useRef(false);
  const revision = useRef(0);

  useEffect(() => {
    if (!active) return;
    let ignore = false;
    listProxyAccounts(provider)
      .then((rows) => {
        if (!ignore) {
          accountsRef.current = rows;
          setAccounts(rows);
        }
      })
      .catch((error) => console.error('Load proxy accounts failed:', error))
      .finally(() => {
        if (!ignore) setLoadedProvider(provider);
      });
    return () => {
      ignore = true;
    };
  }, [provider, active]);

  useEffect(() => {
    if (!active || loadedProvider !== provider) return;
    const account =
      accounts.find(
        (row) => row.source.provider === provider && row.source.accountId === accountId
      ) ?? (!accountId ? accounts.find((row) => row.source.provider === provider) : undefined);
    if (!account) return;
    setForm((form) => {
      if (form.accountSource?.provider !== provider || form.accountSource.accountId !== accountId)
        return form;
      if (!accountId) {
        return {
          ...form,
          name: accountProxyIdentityLabel(account, t),
          accountSource: account.source,
          modelId: '',
          modelIdOptions: [],
        };
      }
      const label = accountProxyIdentityLabel(account, t);
      return form.name !== label ? { ...form, name: label } : form;
    });
  }, [accounts, loadedProvider, provider, accountId, active, setForm, t]);

  const applyModels = useCallback(
    (models: string[]) => {
      setForm((form) => {
        if (form.accountSource?.provider !== provider || form.accountSource.accountId !== accountId)
          return form;
        const labels = accountsRef.current.find(
          (row) => row.source.provider === provider && row.source.accountId === accountId
        )?.modelLabels;
        const options = accountModelPickerOptions(models, form.modelId, labels);
        return {
          ...form,
          modelIdOptions: options,
          modelId: options.includes(form.modelId) ? form.modelId : (options[0] ?? ''),
        };
      });
    },
    [provider, accountId, setForm]
  );
  const fetchModels = useCallback(async () => {
    if (!active || !accountId || fetchingRef.current) return;
    if (voice) {
      const fixed = fixedAccountSpeechModels({ provider, accountId });
      if (fixed !== undefined) {
        applyModels(fixed);
        return;
      }
    }
    const attempt = ++revision.current;
    fetchingRef.current = true;
    setFetching(true);
    try {
      const models = await (voice ? getAccountSpeechModels : getAccountProxyModels)({
        provider,
        accountId,
      });
      if (attempt !== revision.current) return;
      if (
        !voice &&
        (provider === 'qoder' ||
          provider === 'qoderCn' ||
          provider === 'kiro' ||
          provider === 'doubaowork' ||
          provider === 'dola' ||
          provider === 'workBuddy' ||
          provider === 'workBuddyAi' ||
          provider === 'kilo' ||
          provider === 'cline' ||
          provider === 'zcode')
      ) {
        try {
          const rows = await listProxyAccounts(provider);
          if (attempt !== revision.current) return;
          accountsRef.current = rows;
          setAccounts(rows);
        } catch (error) {
          console.error('Load proxy model labels failed:', error);
          return;
        }
        if (attempt !== revision.current) return;
      }
      if (!voice) cacheAccountProxyModels({ provider, accountId }, models);
      applyModels(models);
    } catch (error) {
      if (attempt === revision.current) showToast('error', String(error));
    } finally {
      if (attempt === revision.current) {
        fetchingRef.current = false;
        setFetching(false);
      }
    }
  }, [provider, accountId, active, applyModels, showToast, voice]);

  useEffect(() => {
    const attempt = ++revision.current;
    fetchingRef.current = false;
    void Promise.resolve().then(() => {
      if (attempt !== revision.current) return;
      setFetching(false);
      if (active && accountId && !waitingForLabels) {
        if (voice) {
          const fixed = fixedAccountSpeechModels({ provider, accountId });
          if (fixed !== undefined) applyModels(fixed);
          else void fetchModels();
          return;
        }
        const cached = getCachedAccountProxyModels({ provider, accountId });
        if (cached !== undefined) applyModels(cached);
        else void fetchModels();
      }
    });
    return () => {
      revision.current += 1;
      fetchingRef.current = false;
    };
  }, [provider, accountId, active, waitingForLabels, applyModels, fetchModels, voice]);

  const selectSource = (next: AccountSource) => {
    if (!active) return;
    if (next.provider === provider && next.accountId === accountId) return;
    revision.current += 1;
    fetchingRef.current = false;
    setFetching(false);
    const account = accounts.find(
      (row) => row.source.provider === next.provider && row.source.accountId === next.accountId
    );
    setForm((form) => ({
      ...form,
      name:
        (account ? accountProxyIdentityLabel(account, t) : undefined) ??
        (voice ? accountSpeechProviders : accountProxySources()).find(
          (entry) => entry.id === next.provider
        )?.name ??
        '',
      accountSource: next,
      modelId: '',
      modelIdOptions: [],
    }));
  };
  const savedAccounts = accounts.filter((account) => account.source.provider === provider);
  const selectedAccount = savedAccounts.find((account) => account.source.accountId === accountId);
  const modelLabels = selectedAccount?.modelLabels;
  const providers = (voice ? accountSpeechProviders : accountProxySources()).filter(available);
  return (
    <>
      <div>
        <label className="block text-xs text-cyber-text-secondary mb-1">
          {t('accountProxy.provider')}
        </label>
        <ModelIdCombobox
          selectOnly
          ariaLabel={t('accountProxy.provider')}
          inferFreeFromId={false}
          disabled={!active}
          value={provider}
          options={providers.map((entry) => entry.id)}
          optionLabels={Object.fromEntries(providers.map((entry) => [entry.id, entry.name]))}
          onChange={(value) => selectSource({ provider: value as AccountProvider, accountId: '' })}
        />
      </div>
      <div>
        <label className="block text-xs text-cyber-text-secondary mb-1">
          {t('accountProxy.account')}
        </label>
        <ModelIdCombobox
          selectOnly
          ariaLabel={t('accountProxy.account')}
          inferFreeFromId={false}
          disabled={!active || loading}
          placeholder="—"
          value={
            savedAccounts.some((account) => account.source.accountId === accountId) ? accountId : ''
          }
          options={savedAccounts.map((account) => account.source.accountId)}
          optionLabels={Object.fromEntries(
            savedAccounts.map((account) => {
              const plan = accountProxyPlanLabel(account, t);
              return [
                account.source.accountId,
                `${accountProxyIdentityLabel(account, t)}${plan ? ` · ${plan}` : ''}`,
              ];
            })
          )}
          onChange={(value) => selectSource({ provider, accountId: value })}
        />
      </div>
      <div>
        <label className="block text-xs text-cyber-text-secondary mb-1">{t('model.modelId')}</label>
        <ModelIdCombobox
          selectOnly
          ariaLabel={t('model.modelId')}
          value={modelId}
          options={accountModelPickerOptions(modelIdOptions ?? [], modelId, modelLabels)}
          optionLabels={
            voice
              ? Object.fromEntries(
                  (modelIdOptions ?? []).map((id) => [
                    id,
                    id.split(':').slice(2).join(':') ||
                      accountSpeechProviders.find((entry) => entry.id === provider)?.name ||
                      id,
                  ])
                )
              : modelLabels
          }
          freeModelIds={voice ? undefined : selectedAccount?.freeModelIds}
          freeLabel={t('model.free')}
          placeholder="—"
          disabled={!active || !accountId || waitingForLabels || fetching}
          onChange={(value) => {
            if (!active) return;
            setForm((form) =>
              form.accountSource?.provider === provider &&
              form.accountSource.accountId === accountId
                ? { ...form, modelId: value }
                : form
            );
          }}
          onRefresh={voice && provider !== 'kilo' ? undefined : () => void fetchModels()}
          refreshLabel={t('btn.refresh')}
          refreshing={fetching}
        />
      </div>
    </>
  );
}
