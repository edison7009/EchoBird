import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { syncCatalogs } from '../sync-catalogs.mjs';

test('catalog generation detects drift and rejects invalid install references', () => {
  const root = mkdtempSync(join(tmpdir(), 'echobird-catalog-'));
  const put = (path, value) => {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, JSON.stringify(value));
  };
  try {
    mkdirSync(join(root, 'src/data'), { recursive: true });
    put('docs/api/model-directory/index.json', { version: 1, providers: [] });
    put('docs/api/store/models.json', []);
    put('docs/api/plugins/index.json', { version: 1, plugins: [] });
    put('docs/api/tools/install/index.json', { ids: ['fixture'] });
    put('docs/api/tools/install/fixture.json', { id: 'fixture' });
    put('tools/fixture/paths.json', { paths: { darwin: ['/fixture'] } });
    syncCatalogs(root, true);
    syncCatalogs(root);
    const capabilities = JSON.parse(readFileSync(join(root, 'src/data/toolCapabilities.json')));
    assert.equal(capabilities.tools[0].platforms.macos.nativeVerified, false);
    assert.deepEqual(capabilities.tools[0].platforms.macos.pathHints, ['/fixture']);
    put('src/data/modelDirectory.json', { version: 2 });
    assert.throws(() => syncCatalogs(root), /Stale generated catalog/);
    syncCatalogs(root, true);
    put('docs/api/tools/install/fixture.json', { id: 'different' });
    assert.throws(() => syncCatalogs(root), /Invalid install reference/);
    put('docs/api/tools/install/index.json', { ids: ['fixture', 'fixture'] });
    assert.throws(() => syncCatalogs(root), /Invalid install index/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
