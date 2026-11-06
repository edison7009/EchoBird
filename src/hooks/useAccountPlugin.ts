import { usePluginsStore } from '../stores/pluginsStore';
import { useNavigationStore } from '../stores/navigationStore';
import type { InstalledPlugin } from '../api/plugins';
import { accountProxyProviders } from '../api/accountProxy';
import { useCallback } from 'react';

export const communityAccountApplications = [
  'claudecode',
  'claudedesktop',
  'workbuddy',
  'workbuddyai',
  'doubaowork',
  'dola',
  'trae',
  'traecn',
];

export interface PluginToolProvider {
  application: string;
  applications: string[];
  pluginId: string;
  providerId: string;
  name: string;
  icon: string;
  operations?: string[];
}

/** Applications contributed by enabled plugins, with their account provider. */
export function pluginToolProviders(
  plugins: InstalledPlugin[],
  active: boolean
): PluginToolProvider[] {
  if (!active) return [];
  return plugins
    .filter((plugin) => plugin.enabled)
    .flatMap((plugin) =>
      (plugin.manifest.tools ?? []).flatMap((tool) => {
        const provider = plugin.manifest.echobird.contributes.providers?.find((entry) =>
          entry.applications.includes(tool.id)
        );
        return provider
          ? [
              {
                application: tool.id,
                applications: provider.applications,
                pluginId: plugin.id,
                providerId: provider.id,
                name: provider.title,
                icon: plugin.icon ?? '',
                operations: provider.operations,
              },
            ]
          : [];
      })
    );
}

export function hasAccountPlugin(
  application: string,
  plugins: InstalledPlugin[],
  active: boolean,
  operation?: string
): boolean {
  return (
    active &&
    plugins.some(
      (plugin) =>
        plugin.enabled &&
        (plugin.manifest.echobird.worker?.runtime === 'native' ||
          !!plugin.manifest.tools?.length) &&
        plugin.manifest.echobird.contributes.providers?.some(
          (provider) =>
            provider.applications.includes(application) &&
            (!operation || provider.operations?.includes(operation))
        )
    )
  );
}

/** Built-in account sources plus those contributed by plugin applications. */
export function accountProxySources(): typeof accountProxyProviders {
  return [
    ...accountProxyProviders,
    ...pluginToolProviders(
      usePluginsStore.getState().installed,
      useNavigationStore.getState().developerMode
    )
      .filter((provider) => provider.application === provider.applications[0])
      .map((provider) => ({
        id: `plugin:${provider.application}` as const,
        name: provider.name,
        icon: provider.icon,
      })),
  ];
}

export function accountPluginAvailable(application: string, operation?: string): boolean {
  return hasAccountPlugin(
    application,
    usePluginsStore.getState().installed,
    useNavigationStore.getState().developerMode,
    operation
  );
}

export function useAccountPlugin(application: string, operation?: string): boolean {
  const plugins = usePluginsStore((state) => state.installed);
  const active = useNavigationStore((state) => state.developerMode);
  return hasAccountPlugin(application, plugins, active, operation);
}

export function useAccountProviderFilter() {
  const plugins = usePluginsStore((state) => state.installed);
  const active = useNavigationStore((state) => state.developerMode);
  return useCallback(
    (provider: { id: string }) => {
      if (provider.id.startsWith('plugin:'))
        return hasAccountPlugin(provider.id.slice('plugin:'.length), plugins, active);
      const application = (
        {
          claudeCode: 'claudecode',
          workBuddy: 'workbuddy',
          workBuddyAi: 'workbuddyai',
          doubaowork: 'doubaowork',
          dola: 'dola',
          trae: 'trae',
          traecn: 'traecn',
        } as Record<string, string>
      )[provider.id];
      return !application || hasAccountPlugin(application, plugins, active);
    },
    [plugins, active]
  );
}
