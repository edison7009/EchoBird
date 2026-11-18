import type { LocalTool } from '../../api/types';

const SYSTEM_CATALOG_NAMES = new Map<string, string[]>([
  ['coffeecli', ['Coffee CLI']],
  ['clashverge', ['Clash Verge', 'Clash Verge Rev']],
]);

export const isSystemCatalogTool = (tool: LocalTool) => SYSTEM_CATALOG_NAMES.has(tool.id);

function normalizedName(name: string) {
  return name.toLowerCase().replace(/[\s_-]+/g, '');
}

function matchesPath(detected: string | undefined, path: string | undefined) {
  if (!detected || !path) return false;
  // Windows paths are case insensitive; POSIX paths preserve case.
  const normalize = (value: string) => {
    const normalized = value.replace(/\\/g, '/').replace(/\/$/, '');
    return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
  };
  const actual = normalize(detected);
  const target = normalize(path);
  return actual === target || (target.endsWith('.app') && actual.startsWith(`${target}/`));
}

function matchingSystemApplication(tool: LocalTool, applications: LocalTool[]) {
  const exact = applications.filter((app) => matchesPath(tool.detectedPath, app.detectedPath));
  if (exact.length === 1) return exact[0];
  const names = SYSTEM_CATALOG_NAMES.get(tool.id) ?? [
    tool.name,
    ...Object.values(tool.names ?? {}),
  ];
  const candidates = applications.filter((app) =>
    names.some((name) => normalizedName(name) === normalizedName(app.name))
  );
  return candidates.length === 1 ? candidates[0] : undefined;
}

/** System records already represented by managed My entries keep their AI adapters. */
export function catalogSystemApplicationIds(catalog: LocalTool[], applications: LocalTool[]) {
  return new Set(
    catalog.flatMap((tool) => {
      if (!tool.installed) return [];
      const match = matchingSystemApplication(tool, applications);
      return match?.systemAppId ? [match.systemAppId] : [];
    })
  );
}

/** Preserve catalog IDs/order and install entries, delegating installed copies to the OS. */
export function linkCatalogSystemApplications(catalog: LocalTool[], applications: LocalTool[]) {
  return catalog.map((tool): LocalTool => {
    if (!isSystemCatalogTool(tool)) return tool;
    const match = matchingSystemApplication(tool, applications);
    if (!match?.systemAppId) return tool;
    return {
      ...tool,
      installed: true,
      systemAppId: match.systemAppId,
      detectedPath: match.detectedPath,
      createdAt: match.createdAt,
      icon: match.icon,
      noModelConfig: true,
      noCwdPrompt: true,
      defaultInMyApplications: true,
    };
  });
}
