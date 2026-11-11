import { getOrderedArticles } from './docs-locales.js';
import { siteRoute } from './site-locale.js';

export const orderedArticles = getOrderedArticles('zh');
const normalize = (text) => String(text).normalize('NFKC').toLowerCase().replace(/\s+/g, '');
export const plainText = (html) =>
  html
    .replace(/<[^>]+>/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/\s+/g, ' ')
    .trim();

export function resolveDocRoute(hash, locale = 'zh', pathname = '') {
  const route = siteRoute(pathname, hash);
  const [legacyId, legacySection] = String(hash).replace(/^#/, '').split('/');
  const id = route.locale && route.page === 'docs' ? route.article : legacyId;
  const section = route.locale && route.page === 'docs' ? route.heading : legacySection;
  const articles = getOrderedArticles(locale);
  const article = articles.find((item) => item.id === id) ?? articles[0];
  const heading = article.id === id && section ? section : null;
  return { article, heading };
}

export function searchDocs(query, locale = 'zh') {
  const terms = String(query).trim().split(/\s+/).map(normalize).filter(Boolean);
  if (!terms.length) return [];
  return getOrderedArticles(locale)
    .map((article) => {
      const text = plainText(article.body);
      const corpus = normalize(`${article.title} ${article.summary} ${text}`);
      if (!terms.every((term) => corpus.includes(term))) return null;
      const title = normalize(article.title);
      const summary = normalize(article.summary);
      const score = terms.reduce(
        (total, term) => total + (title.includes(term) ? 12 : summary.includes(term) ? 5 : 1),
        0
      );
      const first = text.toLowerCase().indexOf(String(query).trim().toLowerCase());
      const start = first < 0 ? 0 : Math.max(0, first - 35);
      return {
        article,
        score,
        snippet: `${start ? '…' : ''}${text.slice(start, start + 115)}${text.length > start + 115 ? '…' : ''}`,
      };
    })
    .filter(Boolean)
    .sort((a, b) => b.score - a.score);
}
