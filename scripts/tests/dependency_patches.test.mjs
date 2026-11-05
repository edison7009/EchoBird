import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { verifyAutolinkPatch, verifyDependencyPatches } from '../verify-dependency-patches.mjs';

test('the installed compatibility patch passes; missing patches and changed versions fail', () => {
  verifyDependencyPatches();
  const source = readFileSync(new URL('../../node_modules/mdast-util-gfm-autolink-literal/lib/index.js', import.meta.url), 'utf8');
  assert.throws(() => verifyAutolinkPatch(source + '\n/(?<=^|\\s)email/', '2.0.1'), /missing or outdated/);
  assert.throws(() => verifyAutolinkPatch(source, '2.0.2'), /missing or outdated/);
  assert.throws(() => verifyAutolinkPatch(source.replace(', findUrl]', ', missingUrl]'), '2.0.1'), /missing or outdated/);
});
