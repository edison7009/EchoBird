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

test('keeps the dark text and quota colors on the translucent theme', () => {
  assert.equal(
    rule('.usage-island-theme-translucent'),
    '.usage-island-theme-translucent {\n  --usage-island-bg: rgb(0 0 0 / 0.3);\n'
  );
});
