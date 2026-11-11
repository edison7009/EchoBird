import { docPath, homePath } from './site-locale.js';
import { docsArticles, docsGroups } from './docs-content.js';
import core from './locales/docs-en-core.json' with { type: 'json' };
import desktop from './locales/docs-en-desktop.json' with { type: 'json' };
import help from './locales/docs-en-start-help.json' with { type: 'json' };
const english = new Map([...core, ...desktop, ...help].map((article) => [article.id, article]));
export function getDocsArticles(locale = 'zh') {
  return docsArticles.map((article) =>
    locale === 'en' ? { ...article, ...english.get(article.id) } : article
  );
}
export function getOrderedArticles(locale = 'zh') {
  const articles = getDocsArticles(locale);
  return docsGroups.flatMap((group) => articles.filter((article) => article.group === group));
}
export function localizedDocBody(body, locale) {
  return body
    .replace(
      /href="#([^"/]+)(?:\/([^"]+))?"/g,
      (_, id, heading) => `href="${docPath(locale, id)}${heading ? `#${heading}` : ''}"`
    )
    .replace(/href="\.\/#download"/g, `href="${homePath(locale)}#download"`);
}
