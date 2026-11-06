import { readFileSync, writeFileSync } from 'node:fs';
import { notesForRelease } from './release-notes.mjs';
const [releasePath, canonicalPath, manifestPath] = process.argv.slice(2);
const release = JSON.parse(readFileSync(releasePath, 'utf8'));
const canonical = JSON.parse(readFileSync(canonicalPath, 'utf8'));
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
if (manifest.version !== release.tag_name.replace(/^v/, ''))
  throw new Error('Mirror version changed; refusing a stale notes update');
writeFileSync(
  manifestPath,
  JSON.stringify({ ...manifest, ...notesForRelease(release, canonical) }, null, 2) + '\n'
);
