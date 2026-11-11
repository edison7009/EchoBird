import assert from 'node:assert/strict';
import { test } from 'node:test';
import { docsArticles, docsGroups } from './docs-content.js';
import { orderedArticles, resolveDocRoute, searchDocs } from './docs-model.js';

test('every chapter is navigable exactly once and every local cross-reference resolves', () => {
  assert.equal(new Set(docsArticles.map((article) => article.id)).size, docsArticles.length);
  assert.equal(orderedArticles.length, docsArticles.length);
  for (const article of docsArticles) {
    assert.ok(docsGroups.includes(article.group));
    const headings = [...article.body.matchAll(/<h2 id="([^"]+)"/g)].map((match) => match[1]);
    assert.ok(headings.length > 0, article.id);
    assert.equal(new Set(headings).size, headings.length, article.id);
    for (const [, target] of article.body.matchAll(/href="#([^"]+)"/g)) {
      assert.ok(
        docsArticles.some((item) => item.id === target.split('/')[0]),
        `${article.id}: ${target}`
      );
    }
  }
});
test('deep links preserve a valid chapter and unknown pages safely fall back to the introduction', () => {
  assert.equal(resolveDocRoute('#smart-access/route-endpoints').article.id, 'smart-access');
  assert.equal(resolveDocRoute('#smart-access/route-endpoints').heading, 'route-endpoints');
  assert.equal(resolveDocRoute('#unknown/invalid').article.id, 'welcome');
  assert.equal(resolveDocRoute('#unknown/invalid').heading, null);
});
test('search supports Chinese, spaced model terms, and case-insensitive product names', () => {
  assert.equal(searchDocs('用量岛')[0].article.id, 'usage-island');
  assert.equal(searchDocs('TOKENPET')[0].article.id, 'pets');
  assert.equal(searchDocs('模型 ID')[0].article.id, 'models');
  assert.ok(searchDocs('保存 模型').some((result) => result.article.id === 'models'));
  assert.ok(searchDocs('PATH').some((result) => result.article.id === 'assistant'));
});
test('empty and unmatched search queries do not inject or invent results', () => {
  assert.deepEqual(searchDocs('  '), []);
  assert.deepEqual(searchDocs('不可能存在的功能xyz'), []);
  assert.deepEqual(searchDocs('<script>alert(1)</script>'), []);
});
