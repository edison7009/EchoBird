import { expect, it } from 'vitest';
import type { LocalTool } from '../../api/types';
import {
  catalogSystemApplicationIds,
  linkCatalogSystemApplications,
} from './catalogSystemApplications';

const catalog = (id: string, name: string, detectedPath?: string): LocalTool => ({
  id,
  name,
  detectedPath,
  category: 'Desktop',
  installed: !!detectedPath,
  icon: '/catalog.svg',
  website: 'https://example.test',
  noModelConfig: true,
});
const system = (id: string, name: string, detectedPath: string): LocalTool => ({
  id,
  systemAppId: id,
  name,
  detectedPath,
  category: 'Desktop',
  installed: true,
  noModelConfig: true,
});

it.each([
  [
    'coffeecli',
    'Coffee CLI',
    'Coffee CLI',
    'C:\\Apps\\Coffee CLI\\coffee-cli.exe',
    'C:\\Apps\\Coffee CLI.lnk',
  ],
  [
    'coffeecli',
    'Coffee CLI',
    'Coffee CLI',
    '/Applications/Coffee CLI.app/Contents/MacOS/coffee-cli',
    '/Applications/Coffee CLI.app',
  ],
  [
    'coffeecli',
    'Coffee CLI',
    'coffee-cli',
    '/usr/bin/coffee-cli',
    '/usr/share/applications/coffee-cli.desktop',
  ],
  [
    'clashverge',
    'Clash Verge',
    'Clash Verge Rev',
    'C:\\Apps\\Clash Verge\\Clash Verge.exe',
    'C:\\Apps\\Clash Verge.lnk',
  ],
  [
    'clashverge',
    'Clash Verge',
    'Clash Verge',
    '/Applications/Clash Verge.app/Contents/MacOS/clash-verge',
    '/Applications/Clash Verge.app',
  ],
  [
    'clashverge',
    'Clash Verge',
    'Clash Verge',
    '/usr/bin/clash-verge',
    '/usr/share/applications/clash-verge.desktop',
  ],
])(
  'links %s to native desktop metadata and launch identity (%s / %s)',
  (id, name, nativeName, detected, nativePath) => {
    const original = catalog(id, name, detected);
    const [linked] = linkCatalogSystemApplications(
      [original],
      [system('system:one', nativeName, nativePath)]
    );
    expect(linked).toMatchObject({
      id,
      name,
      installed: true,
      detectedPath: nativePath,
      systemAppId: 'system:one',
      noModelConfig: true,
      noCwdPrompt: true,
      defaultInMyApplications: true,
      website: original.website,
    });
    expect(linked.icon).toBeUndefined();
    expect(original.systemAppId).toBeUndefined();
  }
);

it('prefers the exact detected bundle when multiple copies exist and refuses ambiguous names', () => {
  const copies = [
    system('system:first', 'Coffee CLI', '/Applications/Coffee CLI.app'),
    system('system:second', 'Coffee CLI', '/custom/Coffee CLI.app'),
  ];
  const original = catalog(
    'coffeecli',
    'Coffee CLI',
    '/custom/Coffee CLI.app/Contents/MacOS/coffee-cli'
  );
  expect(linkCatalogSystemApplications([original], copies)[0].systemAppId).toBe('system:second');
  const ambiguous = catalog('coffeecli', 'Coffee CLI');
  expect(linkCatalogSystemApplications([ambiguous], copies)[0]).toBe(ambiguous);
});

it('does not mistake a similarly named product or change unrelated AI applications', () => {
  const original = catalog('coffeecli', 'Coffee CLI');
  const ai = catalog('claudedesktop', 'Claude');
  const entries = [
    system('system:wrong', 'Coffee CLI Helper', '/other.app'),
    system('system:ai', 'Claude', '/Claude.app'),
  ];
  expect(linkCatalogSystemApplications([original, ai], entries)).toEqual([original, ai]);
  expect(linkCatalogSystemApplications([original], [])[0]).toBe(original);
});

it('recognizes a system-only installation while preserving the catalog ID and Store entry', () => {
  const original = catalog('clashverge', 'Clash Verge');
  const [linked] = linkCatalogSystemApplications(
    [original],
    [system('system:one', 'Clash Verge Rev', '/apps/clash.desktop')]
  );
  expect(linked.id).toBe('clashverge');
  expect(linked.installed).toBe(true);
  expect(original.installed).toBe(false);
});

it('selects the installed Coffee bundle among development builds once detection resolves its CLI alias', () => {
  const copies = [
    system('system:installed', 'Coffee CLI', '/Applications/Coffee CLI.app'),
    system(
      'system:debug',
      'Coffee CLI',
      '/Volumes/Development/coffee-cli/target/debug/bundle/macos/Coffee CLI.app'
    ),
    system(
      'system:release',
      'Coffee CLI',
      '/Volumes/Development/coffee-cli/target/release/bundle/macos/Coffee CLI.app'
    ),
  ];
  const alias = catalog('coffeecli', 'Coffee CLI', '/Users/test/.local/bin/coffee-cli');
  // Positive control reproduces the old unresolved discovery result.
  expect(linkCatalogSystemApplications([alias], copies)[0]).toBe(alias);
  const resolved = {
    ...alias,
    detectedPath: '/Applications/Coffee CLI.app/Contents/MacOS/coffee-cli',
  };
  const [linked] = linkCatalogSystemApplications([resolved], copies);
  expect(linked).toMatchObject({
    id: 'coffeecli',
    systemAppId: 'system:installed',
    detectedPath: '/Applications/Coffee CLI.app',
    defaultInMyApplications: true,
  });
});

it('recognizes AI apps in the system list without replacing their My account/model adapters', () => {
  const ai = catalog(
    'claudedesktop',
    'Claude Desktop',
    '/Applications/Claude.app/Contents/MacOS/Claude'
  );
  const records = [system('system:claude', 'Claude', '/Applications/Claude.app')];
  expect([...catalogSystemApplicationIds([ai], records)]).toEqual(['system:claude']);
  expect(linkCatalogSystemApplications([ai], records)[0]).toBe(ai);
  expect([...catalogSystemApplicationIds([{ ...ai, installed: false }], records)]).toEqual([]);
});
