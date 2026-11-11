import { docsGroups } from './docs-content.js';
import { resolveDocRoute, searchDocs } from './docs-model.js';
import { getOrderedArticles, localizedDocBody } from './docs-locales.js';
import { initializeSiteLocale, docPath } from './site-locale.js';
import { uiStrings } from './site-translations.js';
const locale = initializeSiteLocale();
const ui = uiStrings[locale];
const orderedArticles = getOrderedArticles(locale);

const chapters = document.getElementById('doc-chapters');
const body = document.getElementById('doc-body');
const toc = document.querySelector('.doc-toc nav');
const menuButton = document.querySelector('.docs-menu-button');
const backdrop = document.querySelector('.sidebar-backdrop');
const search = document.getElementById('doc-search');
const searchPanel = document.querySelector('.doc-search-panel');
const searchResults = document.getElementById('doc-search-results');
const searchCaption = document.querySelector('.search-caption');
new ResizeObserver(([entry]) =>
  document.documentElement.style.setProperty(
    '--docs-header-height',
    `${Math.ceil(entry.target.getBoundingClientRect().height)}px`
  )
).observe(document.querySelector('.docs-header'));
document.querySelector('.skip-link').addEventListener('click', (event) => {
  event.preventDefault();
  document.getElementById('doc-main').focus();
});
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
let currentArticle = null;
let headings = [];
let forcedHeading = null;
let tocFrame = 0;
let resultLinks = [];
let selectedResult = -1;

function makeLink(href, text, className) {
  const link = document.createElement('a');
  link.href = href;
  link.textContent = text;
  if (className) link.className = className;
  return link;
}
chapters.replaceChildren();
for (const group of docsGroups) {
  const section = document.createElement('section');
  section.className = 'doc-nav-group';
  const title = document.createElement('h2');
  title.textContent = ui.groups[group];
  section.append(title);
  for (const article of orderedArticles.filter((item) => item.group === group))
    section.append(makeLink(docPath(locale, article.id), article.title));
  chapters.append(section);
}

function closeMenu() {
  delete document.body.dataset.menuOpen;
  menuButton.setAttribute('aria-expanded', 'false');
  backdrop.hidden = true;
}
menuButton.addEventListener('click', () => {
  const opening = menuButton.getAttribute('aria-expanded') !== 'true';
  if (!opening) {
    closeMenu();
    return;
  }
  document.body.dataset.menuOpen = 'true';
  menuButton.setAttribute('aria-expanded', 'true');
  backdrop.hidden = false;
});
backdrop.addEventListener('click', closeMenu);
chapters.addEventListener('click', (event) => {
  if (event.target.closest('a')) closeMenu();
});
matchMedia('(min-width: 1001px)').addEventListener('change', (event) => {
  if (event.matches) closeMenu();
});

function updateToc() {
  tocFrame = 0;
  let active = headings[0];
  const headerBottom = document.querySelector('.docs-header').getBoundingClientRect().bottom;
  for (const heading of headings)
    if (heading.getBoundingClientRect().top <= headerBottom + 65) active = heading;
  if (forcedHeading) active = headings.find((item) => item.id === forcedHeading) ?? active;
  for (const link of toc.querySelectorAll('a')) {
    if (active && link.dataset.heading === active.id) link.setAttribute('aria-current', 'location');
    else link.removeAttribute('aria-current');
  }
}
window.addEventListener(
  'scroll',
  () => {
    if (!tocFrame) tocFrame = requestAnimationFrame(updateToc);
  },
  { passive: true }
);
window.addEventListener('resize', updateToc);
for (const type of ['wheel', 'touchstart'])
  window.addEventListener(
    type,
    () => {
      forcedHeading = null;
    },
    { passive: true }
  );
document.addEventListener('keydown', (event) => {
  if (['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End', ' '].includes(event.key))
    forcedHeading = null;
});

function renderRoute(initial = false) {
  const { article, heading } = resolveDocRoute(location.hash, locale, location.pathname);
  if (article !== currentArticle) {
    currentArticle = article;
    document.title = `${article.title} · EchoBird ${ui.docs}`;
    document.getElementById('doc-title').textContent = article.title;
    document.getElementById('doc-group').textContent = ui.groups[article.group];
    document.getElementById('doc-summary').textContent = article.summary;
    // All HTML is authored locally in docs-content.js; search input never enters this markup.
    body.innerHTML = localizedDocBody(article.body, locale);
    headings = [...body.querySelectorAll('h2[id]')];
    toc.replaceChildren();
    for (const item of headings) {
      const link = makeLink(`${docPath(locale, article.id)}#${item.id}`, item.textContent);
      link.dataset.heading = item.id;
      toc.append(link);
    }
    for (const link of chapters.querySelectorAll('a')) {
      if (link.getAttribute('href') === docPath(locale, article.id))
        link.setAttribute('aria-current', 'page');
      else link.removeAttribute('aria-current');
    }
    const pagination = document.querySelector('.doc-pagination');
    pagination.replaceChildren();
    const index = orderedArticles.indexOf(article);
    for (const [other, label, className] of [
      [orderedArticles[index - 1], ui.previous, 'doc-previous'],
      [orderedArticles[index + 1], ui.next, 'doc-next'],
    ]) {
      if (!other) continue;
      const link = makeLink(docPath(locale, other.id), '', className);
      const direction = document.createElement('small');
      direction.textContent = label;
      const title = document.createElement('strong');
      title.textContent = other.title;
      link.append(direction, title);
      pagination.append(link);
    }
    document.getElementById('doc-announcement').textContent = ui.current(article.title);
  }
  closeMenu();
  closeSearch();
  requestAnimationFrame(() => {
    const target = heading ? headings.find((item) => item.id === heading) : null;
    forcedHeading = target?.id ?? null;
    if (target)
      target.scrollIntoView({
        block: 'start',
        behavior: initial || reduced.matches ? 'instant' : 'smooth',
      });
    else window.scrollTo({ top: 0, behavior: 'instant' });
    updateToc();
  });
}

function closeSearch() {
  searchPanel.hidden = true;
  search.setAttribute('aria-expanded', 'false');
  search.removeAttribute('aria-activedescendant');
  selectedResult = -1;
}
function selectResult(index) {
  selectedResult = index;
  resultLinks.forEach((link, i) => {
    link.classList.toggle('is-selected', i === index);
    link.setAttribute('aria-selected', String(i === index));
    if (i === index) link.scrollIntoView({ block: 'nearest', behavior: 'instant' });
  });
  if (resultLinks[index]) search.setAttribute('aria-activedescendant', resultLinks[index].id);
}
function renderSearch() {
  const query = search.value.trim();
  searchResults.replaceChildren();
  resultLinks = [];
  selectedResult = -1;
  if (!query) {
    closeSearch();
    return;
  }
  const matches = searchDocs(query, locale);
  searchCaption.textContent = matches.length ? ui.searchCount(matches.length) : ui.searchEmpty;
  for (const [index, result] of matches.slice(0, 12).entries()) {
    const link = makeLink(docPath(locale, result.article.id), '', 'search-result');
    link.id = `doc-search-result-${index}`;
    link.setAttribute('role', 'option');
    link.setAttribute('aria-selected', 'false');
    const group = document.createElement('small');
    group.textContent = ui.groups[result.article.group];
    const title = document.createElement('strong');
    title.textContent = result.article.title;
    const snippet = document.createElement('span');
    snippet.textContent = result.snippet;
    link.append(group, title, snippet);
    link.addEventListener('click', () => {
      closeSearch();
      if (location.pathname === new URL(link.href).pathname) renderRoute();
    });
    searchResults.append(link);
    resultLinks.push(link);
  }
  searchPanel.hidden = false;
  search.setAttribute('aria-expanded', 'true');
}
search.addEventListener('input', renderSearch);
search.addEventListener('focus', () => {
  if (search.value.trim()) renderSearch();
});
search.addEventListener('keydown', (event) => {
  if (event.key === 'ArrowDown' && resultLinks.length) {
    event.preventDefault();
    selectResult((selectedResult + 1) % resultLinks.length);
  }
  if (event.key === 'ArrowUp' && resultLinks.length) {
    event.preventDefault();
    selectResult((selectedResult - 1 + resultLinks.length) % resultLinks.length);
  }
  if (event.key === 'Enter' && resultLinks.length && !searchPanel.hidden) {
    event.preventDefault();
    resultLinks[Math.max(selectedResult, 0)].click();
    search.blur();
  }
  if (event.key === 'Escape') {
    closeSearch();
    search.blur();
  }
});
document.addEventListener('click', (event) => {
  if (!event.target.closest('.docs-search')) closeSearch();
});
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') {
    const menuWasOpen = document.body.dataset.menuOpen === 'true';
    closeMenu();
    closeSearch();
    if (menuWasOpen) menuButton.focus();
  }
  const typing = event.target.closest('input,textarea,select,[contenteditable="true"]');
  if (
    (!typing && event.key === '/' && !event.ctrlKey && !event.metaKey && !event.altKey) ||
    ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k')
  ) {
    event.preventDefault();
    search.focus();
    search.select();
  }
});
window.addEventListener('hashchange', () => renderRoute());
renderRoute(true);
