import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useRouterCardFade } from './useRouterCardFade';

class Element {
  children: Element[] = [];
  parent: Element | null = null;
  style = { opacity: '1' };
  inert = false;
  attributes = new Map<string, string>();
  classList = { add: vi.fn() };
  animate = vi.fn(() => ({ cancel: vi.fn(), onfinish: null as (() => void) | null }));

  constructor(public text = '') {}
  get firstElementChild() {
    return this.children[0] ?? null;
  }
  get isConnected() {
    return this.parent !== null;
  }
  append(child: Element) {
    child.parent = this;
    this.children.push(child);
  }
  remove() {
    if (this.parent) this.parent.children = this.parent.children.filter((child) => child !== this);
    this.parent = null;
  }
  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }
  querySelectorAll() {
    return [];
  }
  cloneNode() {
    const clone = new Element(this.text);
    clone.style = { ...this.style };
    this.children.forEach((child) => clone.append(child.cloneNode()));
    return clone;
  }
}

describe('router detail crossfade lifecycle', () => {
  let renderer: ReactTestRenderer | undefined;
  afterEach(() => {
    act(() => renderer?.unmount());
    vi.unstubAllGlobals();
  });

  function open(reduced = false) {
    const panel = new Element();
    const content = new Element('First');
    panel.append(content);
    vi.stubGlobal('window', { matchMedia: () => ({ matches: reduced }) });
    vi.stubGlobal('document', { createElement: () => new Element() });
    vi.stubGlobal('getComputedStyle', (node: Element) => node.style);
    function Harness({ id }: { id: string }) {
      const ref = useRouterCardFade(id, true);
      return <div ref={ref} />;
    }
    act(() => {
      renderer = create(<Harness id="first" />, { createNodeMock: () => panel });
    });
    return {
      panel,
      content,
      change(id: string) {
        content.text = id;
        act(() => renderer!.update(<Harness id={id} />));
      },
    };
  }

  it('animates only changes, preserves an interrupted blend, and cleans up on exit', () => {
    const { panel, content, change } = open();
    expect(content.animate).not.toHaveBeenCalled();
    change('Second');
    const overlay = panel.children[1];
    expect(overlay.children[0].text).toBe('First');
    expect(content.text).toBe('Second');
    expect(overlay.inert).toBe(true);
    expect(overlay.attributes.get('aria-hidden')).toBe('true');
    expect(content.animate).toHaveBeenCalledOnce();
    expect(content.animate.mock.calls[0]).toEqual([
      [{ opacity: 0 }, { opacity: 1 }],
      { duration: 180, easing: 'ease-out' },
    ]);
    content.style.opacity = '0.4';
    overlay.style.opacity = '0.6';
    change('Third');
    const blend = panel.children[1];
    expect(panel.children).toHaveLength(2);
    expect(blend.children[0].text).toBe('Second');
    expect(blend.children[0].style.opacity).toBe('0.4');
    expect(blend.children[1].children[0].text).toBe('First');
    expect(blend.children[1].style.opacity).toBe('0.6');
    expect(overlay.animate.mock.results[0].value.cancel).toHaveBeenCalledOnce();
    act(() => renderer!.unmount());
    renderer = undefined;
    expect(panel.children).toHaveLength(1);
    expect(blend.animate.mock.results[0].value.cancel).toHaveBeenCalledOnce();
    expect(content.animate.mock.results[1].value.cancel).toHaveBeenCalledOnce();
  });

  it('keeps reduced-motion changes immediate without overlays', () => {
    const { panel, content, change } = open(true);
    change('Second');
    expect(content.text).toBe('Second');
    expect(panel.children).toHaveLength(1);
    expect(content.animate).not.toHaveBeenCalled();
  });
});
