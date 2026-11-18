import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, mkdirSync, rmSync, copyFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../../', import.meta.url));
function fixture(t, crlf = false) {
  const dir = mkdtempSync(resolve(tmpdir(), 'echobird-pet-renderer-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const manifest = JSON.parse(
    readFileSync(resolve(root, 'public/pet-renderer/build.json'), 'utf8')
  );
  const files = [
    ...Object.keys(manifest.sources),
    ...Object.keys(manifest.outputs).map((name) => `public/pet-renderer/${name}`),
    'public/pet-renderer/build.json',
  ];
  for (const path of files) {
    const target = resolve(dir, path);
    mkdirSync(dirname(target), { recursive: true });
    if (crlf && !path.endsWith('.wasm'))
      writeFileSync(
        target,
        readFileSync(resolve(root, path), 'utf8').replaceAll('\r\n', '\n').replaceAll('\n', '\r\n')
      );
    else copyFileSync(resolve(root, path), target);
  }
  return {
    dir,
    check: () =>
      execFileSync(process.execPath, [resolve(dir, 'scripts/build-pet-renderer.mjs'), '--check'], {
        encoding: 'utf8',
        stdio: 'pipe',
      }),
  };
}
test('artifact verification accepts a CRLF checkout without changing WASM bytes', (t) => {
  const { check } = fixture(t, true);
  assert.match(check(), /sources and packaged artifacts match/);
});
test('artifact verification rejects source drift', (t) => {
  const { dir, check } = fixture(t);
  const path = resolve(dir, 'src-tauri/crates/echo-supply-renderer/src/lib.rs');
  writeFileSync(path, readFileSync(path, 'utf8') + '\n// changed\n');
  assert.throws(check, /sources\/artifacts changed/);
});
test('artifact verification rejects replaced binary assets', (t) => {
  const { dir, check } = fixture(t);
  const path = resolve(dir, 'public/pet-renderer/renderer_bg.wasm');
  const bytes = readFileSync(path);
  bytes[bytes.length - 1] ^= 1;
  writeFileSync(path, bytes);
  assert.throws(check, /sources\/artifacts changed/);
});
