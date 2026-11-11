import assert from 'node:assert/strict';
import { test } from 'node:test';
import { downloadForPlatform, configureDownload, shouldShowChineseMirrors } from './download.js';

for (const [platform, route, extension] of [
  ['Windows', 'windows', '.exe'],
  ['Windows NT 10.0', 'windows', '.exe'],
  ['macOS', 'macos-arm', '.dmg'],
  ['Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)', 'macos-arm', '.dmg'],
  ['Linux', 'linux-deb', '.deb'],
])
  test(`preserves the published choice for ${platform}`, () => {
    assert.equal(downloadForPlatform(platform).route, route);
    assert.equal(downloadForPlatform(platform).extension, extension);
  });

test('uses the modern platform before the user agent without requesting an installer', () => {
  const name = {};
  const extension = { hidden: true };
  const button = {
    href: 'releases',
    querySelector: (selector) =>
      selector === '[data-download-name]' ? name : selector === '.download-ext' ? extension : null,
  };
  configureDownload(button, { userAgentData: { platform: 'Windows' }, userAgent: 'Macintosh' });
  assert.equal(button.href, 'https://echobird.ai/download/windows');
  assert.equal(name.textContent, 'Windows');
  assert.equal(extension.hidden, false);
});

test('unknown platforms retain the release picker', () => {
  const button = { href: 'releases' };
  configureDownload(button, { userAgent: 'Unknown' });
  assert.equal(button.href, 'releases');
});

for (const [platform, file] of [
  ['Windows', 'windows'],
  ['macOS', 'macos'],
  ['Linux', 'linux'],
])
  test(`uses the Coffee CLI website platform icon for ${platform}`, () => {
    let child;
    const icon = {
      replaceChildren: (image) => {
        child = image;
      },
    };
    const button = {
      ownerDocument: { createElement: () => ({}) },
      querySelector: (selector) => (selector === '[data-platform-icon]' ? icon : {}),
    };
    configureDownload(button, { userAgentData: { platform } });
    assert.ok(child.src.endsWith(`platform-${file}.svg`));
    assert.equal(child.alt, '');
  });

test('shows Chinese mirror downloads only for the published Chinese locale rule', () => {
  assert.equal(shouldShowChineseMirrors({ language: 'zh-CN' }), true);
  assert.equal(shouldShowChineseMirrors({ language: 'zh-TW' }), true);
  assert.equal(shouldShowChineseMirrors({ language: 'en-US' }), false);
  assert.equal(shouldShowChineseMirrors({ language: 'zh-CN' }, 'en'), false);
  assert.equal(shouldShowChineseMirrors({ language: 'en-US' }, 'zh-CN'), true);
  assert.equal(shouldShowChineseMirrors({ language: 'en-US' }, 'invalid'), false);
  assert.equal(shouldShowChineseMirrors({}), false);
});
