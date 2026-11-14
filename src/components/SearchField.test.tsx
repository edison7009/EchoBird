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

it('uses the same rem-based toolbar height in apps and plugins and centers the full-height input', async () => {
  const { readFileSync } = await vi.importActual<{
    readFileSync: (path: URL, encoding: string) => string;
  }>('node:fs');
  const styles = readFileSync(new URL('../index.css', import.meta.url), 'utf8');
  const pluginStyles = readFileSync(new URL('../pages/PluginHub.css', import.meta.url), 'utf8');
  const search = styles.match(/\.page-search\s*\{([^}]+)\}/)?.[1];
  const input = styles.match(/\.page-search input\s*\{([^}]+)\}/)?.[1];
  expect(search).toContain('height: 2.25rem;');
  expect(pluginStyles).toContain('--ph-control-height: 2.25rem;');
  expect(input).toContain('height: 100%;');
  expect(input).toContain('padding: 0;');
  expect(input).toContain('line-height: normal;');
});
