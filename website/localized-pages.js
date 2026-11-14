import { homeTranslations, uiStrings } from './site-translations.js';
import { getOrderedArticles, localizedDocBody } from './docs-locales.js';
import { docPath, homePath } from './site-locale.js';
import { siteLanguages, siteLocales, docsLocales, docsLocale } from './site-language-config.js';
const origin = 'https://echobird.ai';
export const escapeHtml = (value) =>
  String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
function replaceRegion(html, id, value) {
  const start = `<!--${id}-start-->`,
    end = `<!--${id}-end-->`;
  if (!html.includes(start) || !html.includes(end))
    throw new Error(`Missing document region ${id}`);
  return html.replace(new RegExp(`${start}[\\s\\S]*?${end}`), `${start}${value}${end}`);
}
function textAt(html, id, value, tag = 'span') {
  return html.replace(
    new RegExp(`(<${tag}[^>]*id="${id}"[^>]*>)[\\s\\S]*?(</${tag}>)`),
    (_, open, close) => open + escapeHtml(value) + close
  );
}
function setSeo(html, locale, page, id, title, description) {
  const canonical = page === 'docs' ? docPath(locale, id) : homePath(locale);
  const alternate = (lang) => origin + (page === 'docs' ? docPath(lang, id) : homePath(lang));
  const language = siteLanguages[locale];
  const alternatives = (page === 'docs' ? docsLocales : siteLocales)
    .map(
      (code) =>
        `<link rel="alternate" hreflang="${siteLanguages[code].tag}" href="${alternate(code)}" />`
    )
    .join('');
  const tags = `<!--site-seo-start--><link rel="canonical" href="${origin + canonical}" />${alternatives}<link rel="alternate" hreflang="x-default" href="${alternate('en')}" /><meta property="og:locale" content="${language.og}" /><meta property="og:title" content="${escapeHtml(title)}" /><meta property="og:description" content="${escapeHtml(description)}" /><!--site-seo-end-->`;
  html = html
    .replace(
      /<html[^>]*>/,
      `<html lang="${language.tag}" dir="${language.dir || 'ltr'}" data-site-locale="${locale}"${page === 'home' ? ' data-button-design="a"' : ''}>`
    )
    .replace(/<title>[\s\S]*?<\/title>/, `<title>${escapeHtml(title)}</title>`);
  html = html
    .replace(/<meta\s+name="description"[^>]*>/g, '')
    .replace('</head>', `<meta name="description" content="${escapeHtml(description)}" /></head>`);
  return html.includes('<!--site-seo-start-->')
    ? html.replace(/<!--site-seo-start-->[\s\S]*?<!--site-seo-end-->/, tags)
    : html.replace('</head>', `${tags}</head>`);
}
function rootAssets(html, locale) {
  return html.replace(/\b(href|src)="([^"]+)"/g, (all, attr, url) => {
    // Vite resolves these placeholders to absolute asset URLs after this hook.
    if (url.startsWith('__VITE_')) return all;
    const doc = /^\/(?:zh\/|en\/)?docs(?:\/([^/#?]+))?\/?([#?].*)?$/.exec(url);
    if (doc) return `${attr}="${docPath(locale, doc[1] || 'welcome')}${doc[2] || ''}"`;
    if (/^\/(zh|en)\/?$/.test(url)) return `${attr}="${homePath(locale)}"`;
    if (/^\/(demo|usage|features)\.html(?:\?|$)/.test(url)) {
      const next = new URL(url.replaceAll('&amp;', '&'), 'https://example.invalid');
      next.searchParams.set('lang', docsLocale(locale));
      return `${attr}="${next.pathname + next.search.replaceAll('&', '&amp;')}"`;
    }
    if (/^(?:https?:|mailto:|data:|#|\/)/.test(url)) return all;
    const raw = url.replace(/^\.\//, '');
    if (raw === '') return `${attr}="${homePath(locale)}"`;
    if (raw === 'docs.html') return `${attr}="${docPath(locale)}"`;
    if (/^(demo|usage|features)\.html(?:\?|$)/.test(raw)) {
      const next = new URL(raw, 'https://example.invalid/');
      next.searchParams.set('lang', docsLocale(locale));
      return `${attr}="${next.pathname + next.search.replaceAll('&', '&amp;')}"`;
    }
    return `${attr}="/${raw}"`;
  });
}
function renderLanguageLink(html, locale, article) {
  let control;
  if (article) {
    const target = locale === 'zh' ? 'en' : 'zh';
    control = `<a class="site-language" data-site-language href="${docPath(target, article)}" lang="${target}" hreflang="${target}">${target === 'en' ? 'English' : '中文'}</a>`;
  } else {
    control = `<details class="site-language-menu" data-site-language-menu><summary class="site-language" aria-label="${escapeHtml(homeTranslations[locale]?.['aria.language'] || (locale === 'zh' ? '选择语言' : 'Choose language'))}">${siteLanguages[locale].label}</summary><div class="site-language-options">${siteLocales.map((code) => `<a class="site-language" data-site-language="${code}" href="${homePath(code)}" dir="auto" lang="${siteLanguages[code].tag}" hreflang="${siteLanguages[code].tag}"${code === locale ? ' aria-current="true"' : ''}>${siteLanguages[code].label}</a>`).join('')}</div></details>`;
  }
  if (html.includes('<!--site-language-start-->'))
    return html.replace(
      /<!--site-language-start-->[\s\S]*?<!--site-language-end-->/,
      `<!--site-language-start-->${control}<!--site-language-end-->`
    );
  return html.replace(
    /<a[^>]*data-site-language[^>]*>[\s\S]*?<\/a\s*>/g,
    `<!--site-language-start-->${control}<!--site-language-end-->`
  );
}

export function renderHome(html, locale = 'zh') {
  if (locale !== 'zh') {
    const translated = homeTranslations[locale];
    html = html.replace(
      /(<([a-z][\w-]*)\b[^>]*\bdata-i18n="([^"]+)"[^>]*>)[\s\S]*?(<\/\2\s*>)/g,
      (all, open, tag, key, close) => {
        if (!Object.hasOwn(translated, key)) throw new Error(`Untranslated home key ${key}`);
        return open + translated[key] + close;
      }
    );
    html = html.replace(/<[^>]+data-i18n-(?:aria-label|title)="[^"]+"[^>]*>/g, (tag) =>
      tag.replace(/(\s)(aria-label|title)="[^"]*"/g, (attr, space, name) => {
        const key = new RegExp(`data-i18n-${name}="([^"]+)"`).exec(tag)?.[1];
        return key ? `${space}${name}="${escapeHtml(translated[key])}"` : attr;
      })
    );
  }
  html = rootAssets(html, locale);
  html = renderLanguageLink(html, locale);
  if (locale === 'zh')
    html = html.replace(/(<div class="cloud-downloads"[^>]*)\bhidden\b([^>]*>)/, '$1$2');
  if (locale !== 'zh')
    html = html.replace(/(<div class="cloud-downloads"[^>]*)(>)/, (_, open, end) =>
      open.includes('hidden') ? open + end : open + ' hidden' + end
    );
  const title =
    locale === 'zh' ? 'EchoBird · 一站式 AI 桌面' : 'EchoBird · All-in-one AI Assistant';
  const description =
    locale === 'zh'
      ? '集中管理 AI 工具、账号与模型，使用智能接入、AI 助手、语音输入和桌面用量岛。'
      : 'Manage AI tools, accounts and models with Smart Routes, an AI assistant, voice input and desktop usage islands.';
  const translated = homeTranslations[locale];
  return setSeo(
    html,
    locale,
    'home',
    null,
    translated?.['seo.title'] || title,
    translated?.['seo.description'] || description
  );
}
export function renderDocs(html, locale = 'zh', id = 'welcome') {
  if (!docsLocales.includes(locale)) throw new Error(`Unsupported document language ${locale}`);
  const ui = uiStrings[locale],
    articles = getOrderedArticles(locale),
    article = articles.find((a) => a.id === id);
  if (!article) throw new Error(`Unknown docs article ${id}`);
  let chapters = '';
  for (const group of Object.keys(ui.groups))
    chapters += `<section class="doc-nav-group"><h2>${ui.groups[group]}</h2>${articles
      .filter((a) => a.group === group)
      .map(
        (a) =>
          `<a href="${docPath(locale, a.id)}"${a.id === id ? ' aria-current="page"' : ''}>${escapeHtml(a.title)}</a>`
      )
      .join('')}</section>`;
  html = replaceRegion(html, 'doc-chapters', chapters);
  html = replaceRegion(html, 'doc-body', localizedDocBody(article.body, locale));
  const headings = [...article.body.matchAll(/<h2 id="([^"]+)">([\s\S]*?)<\/h2>/g)];
  html = replaceRegion(
    html,
    'doc-outline',
    headings
      .map(
        ([, anchor, title]) =>
          `<a href="${docPath(locale, id)}#${anchor}" data-heading="${anchor}">${title}</a>`
      )
      .join('')
  );
  const index = articles.indexOf(article);
  html = replaceRegion(
    html,
    'doc-pagination',
    [
      [articles[index - 1], ui.previous, 'doc-previous'],
      [articles[index + 1], ui.next, 'doc-next'],
    ]
      .filter(([a]) => a)
      .map(
        ([a, label, cl]) =>
          `<a class="${cl}" href="${docPath(locale, a.id)}"><small>${label}</small><strong>${escapeHtml(a.title)}</strong></a>`
      )
      .join('')
  );
  html = textAt(html, 'doc-title', article.title, 'h1');
  html = textAt(html, 'doc-summary', article.summary, 'p');
  html = textAt(html, 'doc-group', ui.groups[article.group]);
  html = html
    .replace(/href="\.\/#download"/g, `href="${homePath(locale)}#download"`)
    .replace(/href="\.\/"/g, `href="${homePath(locale)}"`);
  const words = {
    跳到正文: locale === 'en' ? 'Skip to content' : '跳到正文',
    文档目录: ui.menu,
    文档章节: ui.guide,
    官网导航: locale === 'en' ? 'Website navigation' : '官网导航',
    相邻文档: locale === 'en' ? 'Adjacent articles' : '相邻文档',
    搜索结果: locale === 'en' ? 'Search results' : '搜索结果',
    使用指南: ui.guide,
    使用文档: ui.documentation,
    搜索文档: ui.search,
    '搜索页面、操作或问题': ui.searchPlaceholder,
    本页内容: ui.contents,
    上一章: ui.previous,
    下一章: ui.next,
    反馈文档问题: ui.feedback,
    '联系 EchoBird': ui.contact,
    关闭目录: ui.closeMenu,
    目录: ui.menu,
    文档: ui.docs,
    更多下载: ui.download,
    首页: ui.home,
    '上下选择 · Enter 打开 · Esc 关闭': ui.searchHint,
    选择语言: ui.language,
  };
  // Translate only authored shell text nodes/attributes; article bodies are already localized.
  html = html
    .replace(/>([^<>]+)</g, (all, text) =>
      Object.hasOwn(words, text.trim()) ? `>${words[text.trim()]}<` : all
    )
    .replace(/\b(aria-label|placeholder)="([^"]+)"/g, (all, attr, value) =>
      Object.hasOwn(words, value) ? `${attr}="${words[value]}"` : all
    );
  if (locale === 'zh')
    html = html.replace(/(<div class="cloud-downloads"[^>]*)\bhidden\b([^>]*>)/, '$1$2');
  if (locale === 'en')
    html = html.replace(
      '请启用 JavaScript 以浏览文档目录和正文。你也可以通过 hi@echobird.ai 联系我们。',
      'Enable JavaScript for documentation navigation and search, or contact hi@echobird.ai.'
    );
  html = renderLanguageLink(rootAssets(html, locale), locale, id);
  return setSeo(
    html,
    locale,
    'docs',
    id,
    `${article.title} · EchoBird ${ui.docs}`,
    article.summary
  );
}
