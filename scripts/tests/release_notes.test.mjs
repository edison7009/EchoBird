import { test } from 'node:test';
import assert from 'node:assert/strict';
import { notesForRelease } from '../release-notes.mjs';
test('uses matching canonical localized notes and extracts GitHub language blocks on fallback', () => {
  const release = {
    tag_name: 'v6.2.1',
    published_at: '2026-11-06T00:08:40Z',
    body: '<details><summary>中文</summary>\n中文日志\n</details>\n<details><summary>English</summary>\nEnglish notes\n</details>',
  };
  const notes = { 'zh-cn': '文件日志', en: 'File notes', ja: '日本語' };
  const canonical = {
    version: '6.2.1',
    releaseDate: '2026-11-06',
    releaseNotes: '文件日志',
    releaseNotesI18n: notes,
  };
  assert.deepEqual(notesForRelease(release, canonical).releaseNotesI18n, notes);
  const fallback = notesForRelease(release, { ...canonical, version: '6.2.0' });
  assert.deepEqual(fallback.releaseNotesI18n, { 'zh-cn': '中文日志', en: 'English notes' });
  assert.equal(fallback.releaseDate, '2026-11-06');
  assert.equal(fallback.releaseNotes, '中文日志');
  assert.ok(!JSON.stringify(fallback).includes('<details>'));
});
