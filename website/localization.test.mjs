import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { docsArticles } from './docs-content.js';
import { getDocsArticles } from './docs-locales.js';
import { resolveDocRoute, searchDocs } from './docs-model.js';
import { renderHome, renderDocs } from './localized-pages.js';
import { equivalentLanguageURL, preferredSiteLocale, docPath, homePath } from './site-locale.js';
import { siteLanguages, siteLocales, docsLocales } from './site-language-config.js';
import { homeEnglish, homeTranslations } from './site-translations.js';
import { shouldShowChineseMirrors } from './download.js';

test('every English chapter has a complete translation with the same structure and anchors', () => {
  const english = getDocsArticles('en');
  assert.equal(english.length, 20);
  for (const source of docsArticles) {
    const translated = english.find((a) => a.id === source.id);
    assert.ok(translated);
    assert.notEqual(translated.title, source.title);
    assert.deepEqual(
      [...translated.body.matchAll(/<[^>]+>/g)].map((m) => m[0]),
      [...source.body.matchAll(/<[^>]+>/g)].map((m) => m[0])
    );
    assert.equal(/[\u3400-\u9fff]/.test(translated.body), false, source.id);
  }
});
test('language switching preserves the current page, chapter, anchor and query', () => {
  assert.equal(
    equivalentLanguageURL({ pathname: '/zh/', hash: '#smart-access', search: '?button=a' }, 'en'),
    '/?button=a#smart-access'
  );
  assert.equal(
    equivalentLanguageURL(
      { pathname: '/en/docs/models/', hash: '#model-fields', search: '' },
      'zh'
    ),
    '/zh/docs/models/#model-fields'
  );
  assert.equal(
    equivalentLanguageURL(
      { pathname: '/docs.html', hash: '#smart-access/route-endpoints', search: '' },
      'en'
    ),
    '/docs/smart-access/#route-endpoints'
  );
  assert.equal(docPath('zh'), '\/zh/docs/');
  assert.equal(preferredSiteLocale({ language: 'en-US' }, 'zh-CN'), 'zh');
  assert.equal(preferredSiteLocale({ language: 'zh-CN' }, null), 'zh');
  assert.equal(preferredSiteLocale({ language: 'zh-TW' }, null), 'zh-Hant');
  assert.equal(preferredSiteLocale({ language: 'ja-JP' }, null), 'ja');
  assert.equal(preferredSiteLocale({ language: '' }, null), 'en');
});
test('localized document URLs resolve and search the requested language', () => {
  const route = resolveDocRoute('#model-fields', 'en', '/en/docs/models/');
  assert.equal(route.article.id, 'models');
  assert.equal(route.heading, 'model-fields');
  assert.equal(route.article.title, 'Models');
  assert.equal(searchDocs('TokenPet', 'en')[0].article.id, 'pets');
  assert.ok(searchDocs('account', 'en').length > 0);
});
test('Chinese mirror visibility follows an explicit URL language before other preferences', () => {
  assert.equal(shouldShowChineseMirrors({ language: 'zh-CN' }, 'zh-CN', 'en'), false);
  assert.equal(shouldShowChineseMirrors({ language: 'en-US' }, 'en', 'zh'), true);
  assert.equal(shouldShowChineseMirrors({ language: 'zh-CN' }, 'unknown', null), true);
  assert.equal(shouldShowChineseMirrors({ language: 'zh-CN' }, 'ja', null), false);
});
test('server-rendered home and all docs provide correct SEO, translated content and local links', async () => {
  const home = await readFile(new URL('./index.html', import.meta.url), 'utf8'),
    docs = await readFile(new URL('./docs.html', import.meta.url), 'utf8');
  for (const locale of ['zh', 'en']) {
    const page = renderHome(home, locale);
    assert.ok(page.includes(`https://echobird.ai${homePath(locale)}`));
    assert.ok(page.includes(`href="${docPath(locale)}"`));
    assert.ok(page.includes('hreflang="zh-CN"'));
    assert.ok(page.includes('hreflang="en"'));
    assert.ok(page.includes(`/demo.html?lang=${locale}`));
    assert.equal(/[\u0000-\u0008]/.test(page), false);
    for (const article of docsArticles) {
      const rendered = renderDocs(docs, locale, article.id);
      assert.ok(rendered.includes(`href="https://echobird.ai${docPath(locale, article.id)}"`));
      assert.ok(rendered.includes(`lang="${locale === 'zh' ? 'zh-CN' : 'en'}"`));
      assert.equal(/[\u0000-\u0008]/.test(rendered), false);
      if (locale === 'en') assert.equal(rendered.includes('正在'), false);
    }
  }
  const en = renderHome(renderHome(home, 'zh'), 'en');
  assert.ok(en.includes('Every AI tool.'));
  assert.ok(en.includes('One desktop.'));
  assert.equal(
    (en.match(/class="hero-promise"[\s\S]*?<\/h1>/) || [''])[0].match(/data-i18n="hero.line/g)
      .length,
    2
  );
  assert.equal(en.includes('上点星'), false);
  assert.equal(en.includes('所有 AI 工具'), false);
  assert.ok(en.includes('/docs/'));
  assert.equal(en.includes('/en/'), false);
  assert.equal(en.includes('<select'), false);
  assert.match(en, /data-site-language="zh" href="\/zh\/"/);
  const zh = renderHome(home, 'zh');
  assert.match(zh, /data-site-language="en" href="\/"/);
  for (const rendered of [zh, en, renderDocs(docs, 'en', 'models')]) {
    assert.match(
      rendered,
      /href="https:\/\/github.com\/edison7009\/EchoBird"\s+target="_blank"\s+rel="noopener noreferrer"/
    );
  }
  assert.match(renderDocs(docs, 'en', 'models'), /data-site-language href="\/zh\/docs\/models\/"/);
});

test('HTML localization leaves Vite asset placeholders intact until the bundler resolves them', () => {
  const token = '__VITE_ASSET__favicon__';
  const source = `<html><head><title>EchoBird</title><link rel="icon" href="${token}"></head><body><img src="${token}"></body></html>`;
  const html = renderHome(source, 'en');
  assert.ok(html.includes(`href="${token}"`));
  assert.ok(html.includes(`src="${token}"`));
  assert.equal(html.includes(`/${token}`), false);
});

test('all website languages have complete translated homes and only two document languages', async () => {
  const source = await readFile(new URL('./index.html', import.meta.url), 'utf8');
  assert.equal(siteLocales.length, 16);
  assert.deepEqual(docsLocales, ['zh', 'en']);
  assert.throws(() => renderDocs(source, 'ja'), /Unsupported document language/);
  for (const locale of siteLocales) {
    const html = renderHome(source, locale);
    assert.ok(html.includes(`lang="${siteLanguages[locale].tag}"`), locale);
    assert.ok(html.includes(`dir="${locale === 'ar' ? 'rtl' : 'ltr'}"`), locale);
    assert.ok(
      html.includes(`rel="canonical" href="https://echobird.ai${homePath(locale)}"`),
      locale
    );
    assert.equal((html.match(/data-site-language="/g) || []).length, 16, locale);
    assert.match(html, /<summary[^>]+aria-label="Language \/ /, locale);
    assert.match(html, /class="site-language-label" lang="en">Language<\/span>/, locale);
    assert.match(html, /class="site-language-icon"[^>]+aria-hidden="true"/, locale);
    assert.ok(html.includes(`href="${docPath(locale)}"`), locale);
    assert.ok(
      html.includes(`/demo.html?lang=${locale === 'zh' || locale === 'zh-Hant' ? 'zh' : 'en'}`),
      locale
    );
    if (locale === 'zh') continue;
    const copy = homeTranslations[locale];
    for (const key of Object.keys(homeEnglish))
      assert.ok(typeof copy[key] === 'string' && copy[key].length, `${locale}: ${key}`);
    assert.ok(html.includes(copy['hero.line1']), locale);
    assert.ok(html.includes(copy['usage.body']), locale);
    assert.ok(html.includes(copy['aria.language']), locale);
    assert.equal(shouldShowChineseMirrors({ language: 'zh-CN' }, 'zh-CN', locale), false, locale);
  }
  assert.equal(docPath('ja', 'models'), '/docs/models/');
  assert.equal(docPath('zh-Hant', 'models'), '/zh/docs/models/');
});

test('regional and saved language preferences resolve without dropping browser priority', () => {
  assert.equal(preferredSiteLocale({ languages: ['pt-PT', 'en'] }, null), 'pt-BR');
  assert.equal(preferredSiteLocale({ languages: ['xx-XX', 'ko-KR', 'en'] }, null), 'ko');
  assert.equal(preferredSiteLocale({ language: 'en-US' }, 'ar-SA'), 'ar');
  assert.equal(preferredSiteLocale({ language: 'en-US' }, 'zh-Hans'), 'zh');
  assert.equal(preferredSiteLocale({ language: 'zh-Hans-TW' }, null), 'zh');
  assert.equal(preferredSiteLocale({ language: 'en-US' }, 'zh-HK'), 'zh-Hant');
  assert.equal(preferredSiteLocale({ language: 'fr-CA' }, 'unsupported'), 'fr');
  assert.equal(
    equivalentLanguageURL({ pathname: '/ja/', search: '?a=1', hash: '#download' }, 'pt-BR'),
    '/pt-BR/?a=1#download'
  );
});

test('translated homepage copy preserves authored markup, link targets and HTML entities', () => {
  for (const [locale, copy] of Object.entries(homeTranslations)) {
    for (const [key, original] of Object.entries(homeEnglish)) {
      const markup = (text) =>
        [...text.matchAll(/<[^>]+>|&(?:nbsp|amp|lt|gt|quot);/g)].map((match) => match[0]);
      assert.deepEqual(markup(copy[key]), markup(original), `${locale}: ${key}`);
      assert.doesNotMatch(copy[key], /&(?:nbsp|amp|lt|gt|quot)(?!;)/, `${locale}: ${key}`);
    }
  }
});
