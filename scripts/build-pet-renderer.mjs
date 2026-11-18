// Build the shared Rust/WebGL2 pet and TokenPet renderer. No native IPC per frame.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const crate = resolve(root, 'src-tauri/crates/echo-supply-renderer');
const output = resolve(root, 'public/pet-renderer');
function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(dir, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  });
}
const hash = (path) => {
  const bytes = readFileSync(path);
  // Git checks text out as CRLF on some platforms; only WASM is binary.
  const content = path.endsWith('.wasm') ? bytes : bytes.toString('utf8').replaceAll('\r\n', '\n');
  return createHash('sha256').update(content).digest('hex');
};
const sources = () =>
  Object.fromEntries(
    [resolve(crate, 'Cargo.toml'), ...files(resolve(crate, 'src')), fileURLToPath(import.meta.url)]
      .sort()
      .map((path) => [relative(root, path).replaceAll('\\', '/'), hash(path)])
  );
const outputs = () =>
  Object.fromEntries(
    ['renderer.js', 'renderer_bg.wasm'].map((name) => [name, hash(resolve(output, name))])
  );
if (process.argv.includes('--check')) {
  const expected = JSON.parse(readFileSync(resolve(output, 'build.json'), 'utf8'));
  if (
    JSON.stringify(expected.sources) !== JSON.stringify(sources()) ||
    JSON.stringify(expected.outputs) !== JSON.stringify(outputs())
  ) {
    throw new Error('Pet renderer sources/artifacts changed; run npm run pet-renderer:build.');
  }
  console.log('Pet renderer sources and packaged artifacts match.');
} else {
  const bindgen = process.env.WASM_BINDGEN || 'wasm-bindgen';
  const version = execFileSync(bindgen, ['--version'], { encoding: 'utf8' }).trim();
  if (version !== 'wasm-bindgen 0.2.127')
    throw new Error(`Expected wasm-bindgen 0.2.127, got ${version}`);
  const cargo = ['--manifest-path', resolve(root, 'src-tauri/Cargo.toml')];
  const metadata = JSON.parse(
    execFileSync('cargo', ['metadata', ...cargo, '--no-deps', '--format-version', '1'], {
      encoding: 'utf8',
    })
  );
  execFileSync(
    'cargo',
    [
      'build',
      ...cargo,
      '--locked',
      '-p',
      'echo-supply-renderer',
      '--target',
      'wasm32-unknown-unknown',
      '--release',
    ],
    {
      stdio: 'inherit',
      env: {
        ...process.env,
        CARGO_PROFILE_RELEASE_OPT_LEVEL: 's',
        CARGO_PROFILE_RELEASE_LTO: 'true',
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '1',
        CARGO_PROFILE_RELEASE_PANIC: 'abort',
      },
    }
  );
  mkdirSync(output, { recursive: true });
  execFileSync(
    bindgen,
    [
      '--target',
      'web',
      '--no-typescript',
      '--remove-name-section',
      '--remove-producers-section',
      '--out-dir',
      output,
      '--out-name',
      'renderer',
      resolve(
        metadata.target_directory,
        'wasm32-unknown-unknown/release/echo_supply_renderer.wasm'
      ),
    ],
    { stdio: 'inherit' }
  );
  writeFileSync(
    resolve(output, 'build.json'),
    JSON.stringify({ wasmBindgen: version, sources: sources(), outputs: outputs() }, null, 2) + '\n'
  );
}
