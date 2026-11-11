import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const built = path.join(root, 'build/website');
const output = path.join(root, 'docs');
const manifestPath = path.join(root, 'website/generated-files.json');
async function filesIn(dir, prefix = '') {
  const files = [];
  for (const item of await fs.readdir(dir, { withFileTypes: true })) {
    const relative = path.posix.join(prefix, item.name);
    if (item.isDirectory()) files.push(...(await filesIn(path.join(dir, item.name), relative)));
    else if (item.isFile()) files.push(relative);
  }
  return files.sort();
}
const protectedPath = (file) =>
  /^(?:ccvibe|api|pulse)(?:\/|$)/.test(file) ||
  /^_(?:worker\.js|routes\.json|headers|redirects)$/.test(file) ||
  /^install\.(?:sh|ps1)$/.test(file);
// Validate the build before removing any previously generated files.
const generated = await filesIn(built);
if (generated.some(protectedPath)) throw new Error('Build overlaps retained service resources');
for (const required of ['index.html', 'zh/index.html', 'zh/docs/index.html', 'docs/index.html']) {
  if (!generated.includes(required)) throw new Error(`Incomplete website build: ${required}`);
}
let previous = [];
try {
  previous = JSON.parse(await fs.readFile(manifestPath, 'utf8'));
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
}
for (const file of previous) {
  // Tool icons are also served to native clients; the website must not retire them.
  if (typeof file === 'string' && file.startsWith('icons/')) continue;
  if (
    typeof file !== 'string' ||
    protectedPath(file) ||
    file.startsWith('/') ||
    file.includes('\\') ||
    file.split('/').includes('..')
  )
    throw new Error(`Invalid generated website path: ${file}`);
  await fs.rm(path.join(output, file), { force: true });
}
// Replace old public HTML only. API catalogs, installers, pulse data, vendor assets,
// README screenshots and the retained CC Vibe page are independent of this build.
for (const file of await filesIn(output)) {
  if (file.endsWith('.html') && !protectedPath(file)) await fs.rm(path.join(output, file));
}
for (const file of generated) {
  const dest = path.join(output, file);
  await fs.mkdir(path.dirname(dest), { recursive: true });
  if (file.startsWith('icons/')) {
    // Reuse published native icons; add only assets not already available.
    try {
      await fs.copyFile(path.join(built, file), dest, fs.constants.COPYFILE_EXCL);
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
    }
  } else if (file.endsWith('.html')) {
    // Vite can retain repeated CR characters from source HTML transformations.
    await fs.writeFile(
      dest,
      (await fs.readFile(path.join(built, file), 'utf8')).replace(/\r/g, '')
    );
  } else await fs.copyFile(path.join(built, file), dest);
}
await fs.writeFile(
  manifestPath,
  JSON.stringify(
    generated.filter((file) => !file.startsWith('icons/')),
    null,
    2
  ).replace(/\n/g, '\r\n') + '\r\n'
);
console.log(
  `Prepared ${generated.length} website files in docs/; CC Vibe and service resources retained.`
);
