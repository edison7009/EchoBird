import fs from 'node:fs/promises';
import path from 'node:path';
import { docsArticles } from './docs-content.js';
import { renderHome, renderDocs } from './localized-pages.js';
import { siteLocales, pathLocale, siteRoute, homePath, docPath } from './site-locale.js';
export function localizedRoutesPlugin(root) {
  let outDir;
  let isBuild = false;
  const validArticle = (id) => docsArticles.some((article) => article.id === id);
  const parse = (pathname) => {
    const locale = pathLocale(pathname);
    if (!locale) return null;
    if (pathname === '/') return { locale: 'en', page: 'home' };
    if (/^\/docs(?:\/[^/]+)?\/?$/.test(pathname)) return siteRoute(pathname);
    if (/^\/(zh|en)\/?$/.test(pathname)) return { locale, page: 'home' };
    if (/^\/(zh|en)\/docs(?:\/[^/]+)?\/?$/.test(pathname)) return siteRoute(pathname);
    return { locale, page: 'unknown' };
  };
  const normalize = (req, res) => {
    const url = new URL(req.url, 'http://localhost');
    if (/^\/en(?:\/|$)/.test(url.pathname)) {
      res.statusCode = 301;
      res.setHeader('Location', `${url.pathname.replace(/^\/en/, '') || '/'}${url.search}`);
      res.end();
      return true;
    }
    const route = parse(url.pathname);
    if (route && route.page !== 'unknown' && !url.pathname.endsWith('/')) {
      res.statusCode = 308;
      res.setHeader('Location', `${url.pathname}/${url.search}`);
      res.end();
      return true;
    }
    return false;
  };
  return {
    name: 'echobird-language-paths',
    enforce: 'post',
    configResolved(config) {
      outDir = path.resolve(config.root, config.build.outDir);
      isBuild = config.command === 'build';
    },
    transformIndexHtml(html, ctx) {
      const locale = /data-site-locale="(zh|en)"/.exec(html)?.[1] || (isBuild ? 'zh' : 'en');
      if (ctx.filename === path.join(root, 'index.html')) return renderHome(html, locale);
      if (ctx.filename === path.join(root, 'docs.html')) return renderDocs(html, locale, 'welcome');
      return html;
    },
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        try {
          if (normalize(req, res)) return;
          const pathname = new URL(req.url, 'http://localhost').pathname,
            route = parse(pathname);
          if (!route) return next();
          if (route.page === 'unknown' || (route.page === 'docs' && !validArticle(route.article))) {
            res.statusCode = 404;
            res.setHeader('Content-Type', 'text/html; charset=utf-8');
            res.end(
              `<h1>${route.locale === 'zh' ? '页面不存在' : 'Page not found'}</h1><a href="${docPath(route.locale)}">${route.locale === 'zh' ? '使用文档' : 'Documentation'}</a>`
            );
            return;
          }
          const template = await fs.readFile(
            path.join(root, route.page === 'home' ? 'index.html' : 'docs.html'),
            'utf8'
          );
          const html =
            route.page === 'home'
              ? renderHome(template, route.locale)
              : renderDocs(template, route.locale, route.article);
          res.setHeader('Content-Type', 'text/html; charset=utf-8');
          res.end(await server.transformIndexHtml(pathname, html));
        } catch (error) {
          next(error);
        }
      });
    },
    configurePreviewServer(server) {
      server.middlewares.use((req, res, next) => {
        if (normalize(req, res)) return;
        const route = parse(new URL(req.url, 'http://localhost').pathname);
        if (
          route &&
          (route.page === 'unknown' || (route.page === 'docs' && !validArticle(route.article)))
        ) {
          res.statusCode = 404;
          res.end('Page not found');
          return;
        }
        next();
      });
    },
    async closeBundle() {
      if (!isBuild) return;
      const publicHtml = (html) =>
        html
          .replace(/\r/g, '')
          .replace(/^[ \t]+$/gm, '')
          .replace(/<meta\s+name="robots"\s+content="noindex"\s*\/?>/g, '');
      const clean = (html) => publicHtml(html).replace(/^[ \t]+$/gm, '');
      const home = clean(await fs.readFile(path.join(outDir, 'index.html'), 'utf8')),
        docs = clean(await fs.readFile(path.join(outDir, 'docs.html'), 'utf8'));
      await fs.writeFile(path.join(outDir, 'index.html'), home);
      await fs.writeFile(path.join(outDir, 'docs.html'), docs);
      await fs.writeFile(
        path.join(outDir, '404.html'),
        '<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>404 · EchoBird</title><body style="background:#121312;color:#eee;font:18px system-ui;padding:10vh 8vw"><h1>404</h1><p>Page not found · 页面不存在</p><a style="color:#ee936d" href="/zh/">中文首页</a> · <a style="color:#ee936d" href="/">English home</a></body></html>'
      );
      const sitemapPaths = siteLocales.flatMap((locale) => [
        homePath(locale),
        ...docsArticles.map((article) => docPath(locale, article.id)),
      ]);
      await fs.writeFile(
        path.join(outDir, 'sitemap.xml'),
        `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${[...sitemapPaths, '/ccvibe/'].map((url) => `<url><loc>https://echobird.ai${url}</loc></url>`).join('')}</urlset>
`
      );
      for (const locale of siteLocales) {
        const dir = path.join(outDir, locale === 'zh' ? 'zh' : '');
        await fs.mkdir(dir, { recursive: true });
        await fs.writeFile(path.join(dir, 'index.html'), renderHome(home, locale));
        for (const article of docsArticles) {
          const dest = path.join(dir, 'docs', article.id === 'welcome' ? '' : article.id);
          await fs.mkdir(dest, { recursive: true });
          await fs.writeFile(path.join(dest, 'index.html'), renderDocs(docs, locale, article.id));
        }
      }
      // The root fallback is English; the browser redirects to its saved/preferred locale.
      // Generate it last so Chinese pages always derive from the Chinese source template.
      await fs.writeFile(path.join(outDir, 'index.html'), renderHome(home, 'en'));
      await fs.writeFile(path.join(outDir, 'docs.html'), renderDocs(docs, 'en', 'welcome'));
    },
  };
}
