import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

export function verifyAutolinkPatch(source, version) {
  // Older desktop WebViews cannot parse the upstream lookbehind expression.
  // Pin the expected package shape so upgrades require a deliberate patch review.
  if (
    version !== '2.0.1' ||
    source.includes('(?<=') ||
    source.includes(', findEmail]') ||
    !source.includes(', findUrl]') ||
    !source.includes('function transformGfmAutolinkLiterals(tree)')
  ) {
    throw new Error('Required mdast autolink compatibility patch is missing or outdated. Run npm ci and review patches/mdast-util-gfm-autolink-literal+2.0.1.patch.');
  }
}

export function verifyDependencyPatches(root = fileURLToPath(new URL('../', import.meta.url))) {
  const directory = resolve(root, 'node_modules/mdast-util-gfm-autolink-literal');
  const { version } = JSON.parse(readFileSync(resolve(directory, 'package.json'), 'utf8'));
  verifyAutolinkPatch(readFileSync(resolve(directory, 'lib/index.js'), 'utf8'), version);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  verifyDependencyPatches();
}
