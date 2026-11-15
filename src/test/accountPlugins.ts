import type { InstalledPlugin } from './legacyPluginTypes';
import { create } from 'zustand';
export const usePluginsStore = create<{
  installed: InstalledPlugin[];
  loaded: boolean;
  load: () => Promise<void>;
}>(() => ({ installed: [], loaded: true, load: async () => {} }));

export function installedAccountPlugins(): InstalledPlugin[] {
  return [
    'claudecode',
    'claudedesktop',
    'workbuddy',
    'workbuddyai',
    'doubaowork',
    'dola',
    'trae',
    'traecn',
  ].map((application) => ({
    id: `fixture.${application}`,
    enabled: true,
    directory: `/fixture/${application}`,
    source: 'local',
    revision: null,
    icon: null,
    manifest: {
      name: application,
      publisher: 'fixture',
      version: '0.1.0',
      displayName: application,
      description: 'Account fixture',
      license: 'MIT',
      repository: 'https://example.com/plugins',
      category: 'account',
      echobird: {
        manifestVersion: 1,
        apiVersion: '0.1-draft',
        platforms: ['windows', 'macos', 'linux'],
        permissions: ['account.code'],
        worker: { runtime: 'native', entry: 'bin/account-engine' },
        contributes: {
          pages: [],
          providers: [
            {
              id: application,
              title: application,
              applications: [application],
              operations: ['accounts', 'login.start'],
            },
          ],
        },
      },
    },
  }));
}

export function enableAccountPlugins() {
  // Account services are built in. Older test callers need no plugin setup.
}
