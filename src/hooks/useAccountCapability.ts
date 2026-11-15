import { accountProxyProviders } from '../api/accountProxy';
import { useCallback } from 'react';

export const nativeAccountApplications = [
  'commandcode',
  'claudecode',
  'claudedesktop',
  'workbuddy',
  'workbuddyai',
  'doubaowork',
  'dola',
  'trae',
  'traecn',
];

export function accountProxySources(): typeof accountProxyProviders {
  return accountProxyProviders;
}

export function accountAvailable(application: string, _operation?: string): boolean {
  return nativeAccountApplications.includes(application);
}

export function useAccountCapability(application: string, operation?: string): boolean {
  return accountAvailable(application, operation);
}

export function useAccountProviderFilter() {
  return useCallback((provider: { id: string }) => !provider.id.startsWith('plugin:'), []);
}
