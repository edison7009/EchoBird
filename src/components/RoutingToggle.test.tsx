import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { RoutingToggle } from './RoutingToggle';

describe('RoutingToggle', () => {
  it('keeps an unavailable switch disabled', () => {
    const markup = renderToStaticMarkup(
      <RoutingToggle label="Smart Router" checked={false} disabled onChange={vi.fn()} />
    );
    expect(markup).toContain('role="switch"');
    expect(markup).toContain('aria-checked="false"');
    expect(markup).toContain('disabled=""');
    expect(markup).not.toContain('cursor-wait');
  });

  it('marks a pending switch busy and prevents another change', () => {
    const markup = renderToStaticMarkup(
      <RoutingToggle label="Smart Router" checked pending onChange={vi.fn()} />
    );
    expect(markup).toContain('aria-checked="true"');
    expect(markup).toContain('aria-busy="true"');
    expect(markup).toContain('disabled=""');
  });
});
