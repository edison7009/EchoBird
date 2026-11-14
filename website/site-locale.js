import {
  siteLanguages,
  siteLocales,
  resolveSiteLanguage,
  docsLocale,
} from './site-language-config.js';
export { siteLocales } from './site-language-config.js';
export function pathLocale(pathname) {
  const prefix = pathname.split('/')[1];
  return siteLocales.includes(prefix)
    ? prefix
    : pathname === '/' || /^\/docs(?:\/|$)/.test(pathname)
      ? 'en'
      : null;
}
export function homePath(locale) {
  if (!siteLocales.includes(locale)) throw new Error(`Unknown site language ${locale}`);
  return locale === 'en' ? '/' : `/${locale}/`;
}
export function docPath(locale, id = 'welcome') {
  return `${homePath(docsLocale(locale))}docs/${id === 'welcome' ? '' : `${id}/`}`;
}
export function siteRoute(pathname, hash = '') {
  const locale = pathLocale(pathname);
  const match = /^\/(zh|en)\/docs(?:\/([^/]+))?\/?$/.exec(pathname);
  if (match)
    return {
      locale,
      page: 'docs',
      article: match[2] || 'welcome',
      heading: hash.replace(/^#/, ''),
    };
  const englishDoc = /^\/docs(?:\/([^/]+))?\/?$/.exec(pathname);
  if (englishDoc)
    return {
      locale: 'en',
      page: 'docs',
      article: englishDoc[1] || 'welcome',
      heading: hash.replace(/^#/, ''),
    };
  if (/^\/docs\.html$/.test(pathname)) {
    const [article, heading] = hash.replace(/^#/, '').split('/');
    return { locale, page: 'docs', article: article || 'welcome', heading: heading || '' };
  }
  return { locale, page: 'home', article: null, heading: hash.replace(/^#/, '') };
}
export function equivalentLanguageURL(location, locale) {
  const route = siteRoute(location.pathname, location.hash);
  const path = route.page === 'docs' ? docPath(locale, route.article) : homePath(locale);
  return `${path}${location.search || ''}${route.heading ? `#${route.heading}` : ''}`;
}
export function preferredSiteLocale(browser, saved) {
  const selected = resolveSiteLanguage(saved);
  if (selected) return selected;
  for (const language of browser.languages?.length ? browser.languages : [browser.language]) {
    const locale = resolveSiteLanguage(language);
    if (locale) return locale;
  }
  return 'en';
}
export function initializeSiteLocale() {
  let saved;
  try {
    saved = localStorage.getItem('echobird.lang');
  } catch {
    /* URL and browser preference remain available. */
  }
  const explicit = pathLocale(location.pathname);
  const preferred =
    location.pathname === '/'
      ? preferredSiteLocale(navigator, saved)
      : explicit || preferredSiteLocale(navigator, saved);
  const locale =
    siteRoute(location.pathname, location.hash).page === 'docs' ? docsLocale(preferred) : preferred;
  if (
    !explicit ||
    /^\/en(?:\/|$)/.test(location.pathname) ||
    (location.pathname === '/' && locale !== 'en')
  )
    location.replace(equivalentLanguageURL(location, locale));
  const language = siteLanguages[locale];
  document.documentElement.lang = language.tag;
  document.documentElement.dir = language.dir || 'ltr';
  document.documentElement.dataset.siteLocale = locale;
  for (const link of document.querySelectorAll('[data-site-language]')) {
    const targetLocale = link.dataset.siteLanguage || (locale === 'zh' ? 'en' : 'zh');
    const updateLink = () => {
      link.href = equivalentLanguageURL(location, targetLocale);
    };
    updateLink();
    window.addEventListener('hashchange', updateLink);
    link.addEventListener('click', () => {
      updateLink();
      try {
        localStorage.setItem('echobird.lang', targetLocale);
      } catch {
        /* URLs remain usable. */
      }
    });
  }
  const menu = document.querySelector('[data-site-language-menu]');
  if (menu) {
    document.addEventListener('click', (event) => {
      if (!menu.contains(event.target)) menu.open = false;
    });
    menu.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        menu.open = false;
        menu.querySelector('summary').focus();
      }
    });
  }
  return locale;
}
