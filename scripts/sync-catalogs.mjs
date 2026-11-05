import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function syncCatalogs(root, write = false) {
  const read = (path) => JSON.parse(readFileSync(resolve(root, path), 'utf8'));
  const pairs = [
    ['docs/api/model-directory/index.json', 'src/data/modelDirectory.json'],
    ['docs/api/store/models.json', 'src/data/localModelStore.json'],
    ['docs/api/plugins/index.json', 'src/data/pluginIndex.json'],
  ];
  function generated(path, value) {
    if (write) writeFileSync(resolve(root, path), JSON.stringify(value, null, 2) + '\n');
    else if (JSON.stringify(read(path)) !== JSON.stringify(value))
      throw new Error(`Stale generated catalog: ${path}. Run npm run catalog:generate.`);
  }
  for (const [source, target] of pairs) generated(target, read(source));
  const ids = read('docs/api/tools/install/index.json').ids;
  if (!Array.isArray(ids) || new Set(ids).size !== ids.length)
    throw new Error('Invalid install index');
  for (const id of ids) {
    if (!/^[a-z0-9-]+$/.test(id) || read(`docs/api/tools/install/${id}.json`).id !== id)
      throw new Error(`Invalid install reference: ${id}`);
  }
  // Catalog declarations are evidence of path hints, never a claim of native verification/vendor availability.
  const tools = readdirSync(resolve(root, 'tools'), { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .flatMap((entry) => {
      let paths;
      try {
        paths = read(`tools/${entry.name}/paths.json`);
      } catch (error) {
        if (error.code === 'ENOENT') return [];
        throw error;
      }
      return [
        {
          id: entry.name,
          apiProtocols: paths.apiProtocol ?? [],
          installReference: ids.includes(entry.name),
          platforms: Object.fromEntries(
            [
              ['windows', 'win32'],
              ['macos', 'darwin'],
              ['linux', 'linux'],
            ].map(([os, key]) => [
              os,
              { pathHints: paths.paths?.[key] ?? [], nativeVerified: false },
            ])
          ),
        },
      ];
    })
    .sort((a, b) => a.id.localeCompare(b.id));
  generated('src/data/toolCapabilities.json', { schemaVersion: 1, tools });
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  syncCatalogs(fileURLToPath(new URL('../', import.meta.url)), process.argv.includes('--write'));
}
