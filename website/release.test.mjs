import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { test } from 'node:test';
import { docsArticles } from './docs-content.js';
import { siteLocales, docsLocales } from './site-language-config.js';
import { docPath, homePath } from './site-locale.js';
const output = new URL('../docs/', import.meta.url);
const read = (file) => fs.readFile(new URL(file, output), 'utf8');

test('public release contains indexable localized homes and all chapters with resolvable assets and links', async () => {
  assert.match(await read('index.html'), /<html lang="en"/);
  assert.match(await read('docs.html'), /<html lang="en"/);
  for (const locale of siteLocales) {
    const routes = [
      homePath(locale),
      ...(docsLocales.includes(locale)
        ? docsArticles.map((article) => docPath(locale, article.id))
        : []),
    ];
    for (const route of routes) {
      const html = await read(`${route.slice(1)}index.html`);
      assert.equal(html.includes('content="noindex"'), false, route);
      assert.match(html, /rel="icon"[^>]*href="\/assets\/icon-[^"]+\.png"/, route);
      assert.ok(html.includes(`href="https://echobird.ai${route}"`), route);
      assert.equal(html.includes('/src/website/'), false, route);
      assert.equal(/(?:href|src)="\/\/assets\//.test(html), false, route);
      for (const [, raw] of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
        const url = new URL(raw.replaceAll('&amp;', '&'), `https://echobird.ai${route}`);
        if (url.origin !== 'https://echobird.ai' || url.pathname.startsWith('/download/')) continue;
        const dest = url.pathname.endsWith('/') ? `${url.pathname}index.html` : url.pathname;
        await assert.doesNotReject(fs.access(new URL(dest.slice(1), output)), `${route} → ${raw}`);
      }
    }
  }
});

test('obsolete pages are retired, while CC Vibe and download service resources remain', async () => {
  for (const file of [
    'openrouter/index.html',
    'ai-app-store/index.html',
    'ai-app-store/zh/index.html',
    'ai-game-store/index.html',
    'ai-game-store/zh/index.html',
    'support/index.html',
    'legal/privacy/index.html',
    'legal/terms/index.html',
    'legal/refund/index.html',
  ]) {
    await assert.rejects(fs.access(new URL(file, output)), { code: 'ENOENT' });
  }
  for (const file of [
    'ccvibe/index.html',
    'ccvibe/step1.webp',
    'assets/vendor/fonts.css',
    '_worker.js',
    '_routes.json',
    'api/version/index.json',
    'install.sh',
    'install.ps1',
    '404.html',
  ]) {
    await fs.access(new URL(file, output));
  }
  const routes = JSON.parse(await read('_routes.json'));
  assert.deepEqual(routes.include, ['/download/*']);
  const sitemap = await read('sitemap.xml');
  assert.equal((sitemap.match(/<loc>/g) || []).length, 57);
  assert.ok(sitemap.includes('https://echobird.ai/ccvibe/'));
  assert.equal(sitemap.includes('/en/'), false);
  assert.ok((await read('_redirects')).includes('/en/* /:splat 301'));
  assert.equal(/\/legal\/|\/openrouter\/|\/ai-app-store\//.test(sitemap), false);
});

test('the new favicon preserves the previous website icon byte for byte', async () => {
  assert.deepEqual(
    await fs.readFile(new URL('./assets/icon.png', import.meta.url)),
    await fs.readFile(new URL('icon.png', output))
  );
});

test('additional website languages do not generate translated document trees', async () => {
  for (const locale of siteLocales.filter((code) => !docsLocales.includes(code))) {
    await fs.access(new URL(`${locale}/index.html`, output));
    await assert.rejects(fs.access(new URL(`${locale}/docs/index.html`, output)), {
      code: 'ENOENT',
    });
  }
});
