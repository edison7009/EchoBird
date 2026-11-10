import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

// Vitest replaces stylesheets with empty strings, so the agreed visuals are pinned here.
const css = readFileSync(
  new URL('../../src/pages/UsageIsland/UsageIsland.css', import.meta.url),
  'utf8'
).replace(/\r\n/g, '\n');
const rule = (selector) => {
  const start = css.indexOf(`${selector} {`);
  assert.ok(start >= 0, selector);
  return css.slice(start, css.indexOf('}', start));
};

test('draws what remains counterclockwise from 12 o’clock', () => {
  assert.match(rule('.usage-island-gauge svg'), /transform: rotate\(-90deg\) scaleY\(-1\);/);
});

test('tints the empty ring and bar with the quota color, like the account quota bars', () => {
  assert.ok(
    rule(
      ".usage-island :is([data-tone='healthy'], [data-tone='low'], [data-tone='critical'])"
    ).includes('--usage-track: color-mix(in srgb, var(--usage-color) 20%, transparent);')
  );
  assert.ok(
    rule(".usage-island [data-tone='unknown']").includes(
      '--usage-track: var(--usage-island-track);'
    )
  );
  for (const selector of ['.usage-island-gauge-track', '.usage-island-line-track'])
    assert.ok(rule(selector).includes('var(--usage-track, var(--usage-island-track))'), selector);
});

test('uses the explicit opacity surface and removes the usage-island translucent theme', () => {
  const css = readFileSync(
    new URL('../../src/pages/UsageIsland/UsageIsland.css', import.meta.url),
    'utf8'
  );
  assert.match(css, /var\(--usage-island-surface-opacity, 100%\)/);
  // The island must inherit the opacity the track sets inline, not shadow it.
  assert.ok(!/--usage-island-surface-opacity:/.test(css));
  assert.match(css, /background: color-mix\(\s*in srgb,\s*var\(--usage-island-bg\)/);
  assert.ok(!css.includes('usage-island-theme-translucent'));
  const shared = readFileSync(new URL('../../src/index.css', import.meta.url), 'utf8');
  assert.ok(!shared.includes('.usage-island.usage-island-theme-translucent'));
  assert.ok(css.includes(".usage-island-gauge[data-smart-active='true'] .usage-island-icon"));
  assert.match(css, /background-size: 20px 20px/);
  assert.match(css, /--usage-island-smart-light: rgb\(var\(--accent-rgb\) \/ 0\.5\)/);
  assert.match(css, /background-position: 20px 0/);
  assert.ok(!css.includes('usage-island-smart-stripes'));
  assert.ok(
    css.includes(
      ".usage-island[data-layout='list'] .usage-island-tool[data-smart-active='true']::before"
    )
  );
  assert.ok(
    css.includes(
      ".usage-island[data-style='line'] .usage-island-tool[data-smart-active='true']::before"
    )
  );
});
