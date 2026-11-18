import { act, create } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { SearchField } from './SearchField';

it('shares the plugin search field and sends input and clear actions through one callback', () => {
  const onChange = vi.fn();
  const renderer = create(
    <SearchField value="App" onChange={onChange} label="Search apps" clearLabel="Clear" />
  );
  try {
    expect(renderer.root.findByProps({ className: 'page-search' })).toBeDefined();
    expect(renderer.root.findByType('input').props['aria-label']).toBe('Search apps');
    act(() => renderer.root.findByType('input').props.onChange({ target: { value: 'Other' } }));
    expect(onChange).toHaveBeenLastCalledWith('Other');
    act(() => renderer.root.findByProps({ 'aria-label': 'Clear' }).props.onClick());
    expect(onChange).toHaveBeenLastCalledWith('');
  } finally {
    act(() => renderer.unmount());
  }
});

it('uses the rem-based toolbar height in apps and centers the full-height input', async () => {
  const { readFileSync } = await vi.importActual<{
    readFileSync: (path: URL, encoding: string) => string;
  }>('node:fs');
  const styles = readFileSync(new URL('../index.css', import.meta.url), 'utf8');
  const search = styles.match(/\.page-search\s*\{([^}]+)\}/)?.[1];
  const input = styles.match(/\.page-search input\s*\{([^}]+)\}/)?.[1];
  expect(search).toContain('height: 2.25rem;');
  expect(input).toContain('height: 100%;');
  expect(input).toContain('padding: 0;');
  expect(input).toContain('line-height: normal;');
});

it('keeps input and clear locked during submission without changing search styling', () => {
  const onChange = vi.fn();
  const renderer = create(
    <SearchField disabled value="model" onChange={onChange} label="Model ID" clearLabel="Clear" />
  );
  try {
    const input = renderer.root.findByType('input');
    const clear = renderer.root.findByType('button');
    expect(input.props.disabled).toBe(true);
    expect(clear.props.disabled).toBe(true);
    act(() => input.props.onChange({ target: { value: 'different' } }));
    act(() => clear.props.onClick());
    expect(onChange).not.toHaveBeenCalled();
    expect(renderer.root.findByProps({ className: 'page-search' })).toBeDefined();
  } finally {
    act(() => renderer.unmount());
  }
});

it('reserves the clear-button slot when empty and removes the hidden control from keyboard actions', () => {
  const onChange = vi.fn();
  const render = (value: string) => (
    <SearchField value={value} onChange={onChange} label="Search" clearLabel="Clear" />
  );
  const renderer = create(render(''));
  try {
    const clear = renderer.root.findByType('button');
    expect(clear.props.className).toContain('invisible');
    expect(clear.props['aria-hidden']).toBe(true);
    expect(clear.props.disabled).toBe(true);
    expect(clear.props.tabIndex).toBe(-1);
    act(() => clear.props.onClick());
    expect(onChange).not.toHaveBeenCalled();
    act(() => renderer.update(render('model')));
    expect(renderer.root.findByType('button')).toBe(clear);
    expect(clear.props.className).not.toContain('invisible');
    expect(clear.props.disabled).toBe(false);
    act(() => clear.props.onClick());
    expect(onChange).toHaveBeenCalledExactlyOnceWith('');
    act(() => renderer.update(render('')));
    expect(renderer.root.findByType('button')).toBe(clear);
    expect(clear.props.className).toContain('invisible');
  } finally {
    act(() => renderer.unmount());
  }
});
