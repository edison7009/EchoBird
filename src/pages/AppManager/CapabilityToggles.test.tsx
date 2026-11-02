import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import renderer, { act } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CapabilityToggles, type CapabilityToggle } from './CapabilityToggles';

vi.mock('react-dom', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-dom')>()),
  createPortal: (node: React.ReactNode) => node,
}));

const toggles = (onChange: CapabilityToggle['onChange'] = vi.fn()): CapabilityToggle[] => [
  { label: 'Web Search', hint: 'search hint', checked: true, onChange },
  { label: '1M', hint: 'context hint', checked: false, onChange },
];

const glyphs = (markup: string) => markup.match(/>\?<\/span>/g) ?? [];

afterEach(() => vi.unstubAllGlobals());

describe('CapabilityToggles', () => {
  it('keeps each button independently pressed without a switch or sliding group', () => {
    const markup = renderToStaticMarkup(<CapabilityToggles toggles={toggles()} />);
    expect(markup).toMatch(/aria-pressed="true" aria-label="Web Search"/);
    expect(markup).toMatch(/aria-pressed="false" aria-label="1M"/);
    expect(markup).not.toContain('role="switch"');
    expect(markup).not.toContain('view-mode-tabs');
    expect(markup).not.toContain('cursor-');
  });

  it('toggles only the clicked capability to its opposite value', () => {
    const first = vi.fn();
    const second = vi.fn();
    const [webSearch, oneM] = toggles();
    const tree = renderer.create(
      <CapabilityToggles
        toggles={[
          { ...webSearch, onChange: first },
          { ...oneM, onChange: second },
        ]}
      />
    );
    act(() => tree.root.findByProps({ 'aria-label': '1M' }).props.onClick());
    expect(second).toHaveBeenCalledWith(true);
    expect(first).not.toHaveBeenCalled();
    act(() => tree.root.findByProps({ 'aria-label': 'Web Search' }).props.onClick());
    expect(first).toHaveBeenCalledWith(false);
  });

  it('disables every button while pending', () => {
    const markup = renderToStaticMarkup(<CapabilityToggles toggles={toggles()} disabled />);
    expect(markup.match(/<button[^>]*disabled=""/g)).toHaveLength(2);
  });

  it('merges multiple hints into one labelled glyph', () => {
    const markup = renderToStaticMarkup(<CapabilityToggles toggles={toggles()} />);
    expect(glyphs(markup)).toHaveLength(1);
    expect(markup).toContain('aria-label="Web Search: search hint 1M: context hint"');
    expect(markup).not.toContain('role="tooltip"');

    vi.stubGlobal('document', { body: {} });
    vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
    const tree = renderer.create(<CapabilityToggles toggles={toggles()} />);
    const anchor = tree.root.find((node) => node.props.onMouseEnter && node.type === 'span');
    act(() => anchor.props.onMouseEnter());
    const tooltip = tree.root.findByProps({ role: 'tooltip' });
    for (const text of ['Web Search', 'search hint', '1M', 'context hint']) {
      expect(tooltip.findAll((node) => node.children.includes(text))).not.toHaveLength(0);
    }
  });

  it('keeps a lone hint unchanged', () => {
    const markup = renderToStaticMarkup(<CapabilityToggles toggles={toggles().slice(0, 1)} />);
    expect(glyphs(markup)).toHaveLength(1);
    expect(markup).toContain('aria-label="search hint"');
  });
});
