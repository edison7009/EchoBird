import { useAccountProviderFilter } from '../../hooks/useAccountCapability';
import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useNavigationStore } from '../../stores/navigationStore';
import { useVoiceInputStore, voiceBusy } from '../../stores/voiceInputStore';
import { getVoiceState, type VoiceSnapshot } from '../../api/voiceInput';
import type { AccountProvider } from '../../api/types';
import {
  accountSpeechProviders,
  listProxyAccounts,
  type ProxyAccount,
} from '../../api/accountProxy';

interface AccountDirectory {
  accounts: ProxyAccount[];
  accountCounts: Partial<Record<AccountProvider, number>>;
}

const Context = createContext<AccountDirectory | null>(null);

export function VoiceInputProvider({ children }: { children: ReactNode }) {
  const available = useAccountProviderFilter();
  const active = useNavigationStore((state) => state.activePage === 'voice');
  const [directory, setDirectory] = useState<AccountDirectory>({
    accounts: [],
    accountCounts: {},
  });
  useEffect(() => {
    if (!active) return;
    let disposed = false;
    void Promise.resolve().then(async () => {
      if (disposed) return;
      const providers = accountSpeechProviders.filter(available);
      const results = await Promise.allSettled(
        providers.map((provider) => listProxyAccounts(provider.id))
      );
      if (disposed) return;
      setDirectory((previous) => {
        const accountCounts = { ...previous.accountCounts };
        const accounts = providers.flatMap((provider, index) => {
          const result = results[index];
          if (result.status === 'fulfilled') {
            accountCounts[provider.id] = result.value.length;
            return result.value;
          }
          return previous.accounts.filter((account) => account.source.provider === provider.id);
        });
        return { accounts, accountCounts };
      });
    });
    return () => {
      disposed = true;
    };
  }, [active, available]);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<VoiceSnapshot>('voice-input-state', (event) =>
      useVoiceInputStore.getState().accept(event.payload)
    )
      .then((stop) => {
        if (disposed) stop();
        else {
          unlisten = stop;
          void getVoiceState()
            .then((snapshot) => {
              if (!disposed) useVoiceInputStore.getState().accept(snapshot);
            })
            .catch(() => {});
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  useEffect(() => {
    if (!active) return;
    void useVoiceInputStore.getState().load();
    return () => {
      const state = useVoiceInputStore.getState();
      if (
        ['page', 'test'].includes(state.snapshot?.origin ?? '') &&
        voiceBusy(state.snapshot?.phase)
      )
        void state.cancel();
    };
  }, [active]);
  return (
    <Context.Provider
      value={{
        ...directory,
        accounts: directory.accounts.filter((account) =>
          available({ id: account.source.provider })
        ),
      }}
    >
      {children}
    </Context.Provider>
  );
}

export function useVoiceAccounts() {
  const value = useContext(Context);
  if (!value) throw new Error('VoiceInputProvider is required');
  return value;
}
