export const siteLocales = ['zh', 'en'];
export function pathLocale(pathname) {
  return (
    /^\/(zh|en)(?:\/|$)/.exec(pathname)?.[1] ??
    (pathname === '/' || /^\/docs(?:\/|$)/.test(pathname) ? 'en' : null)
  );
}
export function homePath(locale) {
  return locale === 'en' ? '/' : '/zh/';
}
export function docPath(locale, id = 'welcome') {
  return `${homePath(locale)}docs/${id === 'welcome' ? '' : `${id}/`}`;
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
  if (saved === 'zh-CN' || saved === 'zh') return 'zh';
  if (saved === 'en') return 'en';
  return /^zh/i.test(browser.language || '') ? 'zh' : 'en';
}
export function initializeSiteLocale() {
  let saved;
  try {
    saved = localStorage.getItem('echobird.lang');
  } catch {
    /* URL and browser preference remain available. */
  }
  const explicit = pathLocale(location.pathname);
  const locale =
    location.pathname === '/'
      ? preferredSiteLocale(navigator, saved)
      : explicit || preferredSiteLocale(navigator, saved);
  if (
    !explicit ||
    /^\/en(?:\/|$)/.test(location.pathname) ||
    (location.pathname === '/' && locale === 'zh')
  )
    location.replace(equivalentLanguageURL(location, locale));
  document.documentElement.lang = locale === 'zh' ? 'zh-CN' : 'en';
  document.documentElement.dataset.siteLocale = locale;
  const targetLocale = locale === 'zh' ? 'en' : 'zh';
  for (const link of document.querySelectorAll('[data-site-language]')) {
    link.textContent = targetLocale === 'en' ? 'English' : '中文';
    link.lang = targetLocale;
    link.hreflang = targetLocale;
    const updateLink = () => {
      link.href = equivalentLanguageURL(location, targetLocale);
    };
    updateLink();
    window.addEventListener('hashchange', updateLink);
    link.addEventListener('click', () => {
      updateLink();
      try {
        localStorage.setItem('echobird.lang', targetLocale === 'zh' ? 'zh-CN' : 'en');
      } catch {
        /* Language URLs work without storage. */
      }
    });
  }
  return locale;
}
