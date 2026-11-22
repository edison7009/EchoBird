import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AppLibraryPanel, AppLibraryTabs, libraryKinds } from './AppLibraryPanel';
import { AppPanelNotice } from './AppPanelNotice';
import { appLibrary, type LibraryEntry } from '../../api/appLibrary';
import { useAppLibraryStore } from '../../stores/appLibraryStore';
import { useToolsStore } from '../../stores/toolsStore';
import type { LocalTool } from '../../api/types';

vi.mock('../../api/appLibrary', async (original) => ({
  ...(await original<typeof import('../../api/appLibrary')>()),
  appLibrary: { list: vi.fn(), copy: vi.fn(), remove: vi.fn(), toggle: vi.fn(), addMcp: vi.fn() },
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));

const tool = (id: string, name: string, installed = true): LocalTool => ({
  id,
  name,
  installed,
  category: 'CLI Code',
});

const resource = (name: string): LibraryEntry => ({
  id: `0/${name}`,
  name,
  description: null,
  group: null,
  path: null,
  enabled: null,
  canCopy: false,
  canRemove: false,
  canToggle: false,
});

function deferredRead() {
  let resolve!: (entries: LibraryEntry[]) => void;
  const promise = new Promise<LibraryEntry[]>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe('application resources', () => {
  let renderer: ReactTestRenderer;
  const claude = tool('claudecode', 'Claude Code');

  beforeEach(() => {
    vi.resetAllMocks();
    useAppLibraryStore.setState({ snapshots: {} });
    useToolsStore
      .getState()
      .setDetectedTools([
        claude,
        tool('codex', 'Codex'),
        tool('chatgptdesktop', 'ChatGPT'),
        tool('cursor', 'Cursor', false),
      ]);
  });

  afterEach(() => {
    act(() => renderer.unmount());
    useToolsStore.getState().setDetectedTools([]);
    vi.useRealTimers();
  });

  it('keeps Model first and renders the supported text tabs without scanning', () => {
    const select = vi.fn();
    act(() => {
      renderer = create(
        <AppLibraryTabs kinds={libraryKinds(claude.id)} active="model" onSelect={select} />
      );
    });
    const tabs = renderer.root.findAllByProps({ role: 'tab' });
    expect(tabs.map((tab) => tab.children)).toEqual(
      ['model', 'skills', 'mcp', 'plugins', 'memory'].map((kind) => [`appLibrary.${kind}`])
    );
    expect(tabs[0].props['aria-selected']).toBe(true);
    act(() => tabs[1].props.onClick());
    expect(select).toHaveBeenCalledWith('skills');
    expect(appLibrary.list).not.toHaveBeenCalled();
  });

  it('moves the same underline to the selected tab and supports keyboard selection', () => {
    const kinds = libraryKinds(claude.id);
    const onSelect = vi.fn();
    const focus = vi.fn();
    const firstTab = {
      getBoundingClientRect: () => ({ left: 20, right: 60, width: 40 }),
      focus,
    };
    const secondTab = {
      getBoundingClientRect: () => ({ left: 72, right: 116, width: 44 }),
      focus,
    };
    let selected = firstTab;
    const track = {
      querySelector: () => selected,
      querySelectorAll: () => [firstTab, secondTab],
      getBoundingClientRect: () => ({ left: 20 }),
    };
    const viewport = {
      getBoundingClientRect: () => ({ left: 20, right: 100 }),
      scrollLeft: 0,
    };
    const underline = {
      getBoundingClientRect: vi.fn(),
      style: { removeProperty: vi.fn() } as Partial<CSSStyleDeclaration>,
    };
    act(() => {
      renderer = create(<AppLibraryTabs kinds={kinds} active="model" onSelect={onSelect} />, {
        createNodeMock: (element) =>
          element.props.role === 'tablist' ? track : element.type === 'span' ? underline : viewport,
      });
    });
    expect(underline.style.width).toBe('40px');
    expect(underline.style.transform).toBe('translateX(0px)');
    expect(underline.style.removeProperty).toHaveBeenCalledExactlyOnceWith('transition');

    const preventDefault = vi.fn();
    act(() =>
      renderer.root.findAllByProps({ role: 'tab' })[0].props.onKeyDown({
        key: 'ArrowRight',
        preventDefault,
      })
    );
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(focus).toHaveBeenCalledWith({ preventScroll: true });
    expect(onSelect).toHaveBeenCalledWith('skills');

    selected = secondTab;
    act(() =>
      renderer.update(<AppLibraryTabs kinds={kinds} active="skills" onSelect={onSelect} />)
    );
    expect(underline.style.width).toBe('44px');
    expect(underline.style.transform).toBe('translateX(52px)');
    expect(viewport.scrollLeft).toBe(16);
    expect(underline.style.removeProperty).toHaveBeenCalledOnce();
  });

  it('exposes the user resources supported by Pi and Qoder', () => {
    expect(libraryKinds('pi')).toEqual(['skills', 'mcp', 'plugins', 'memory']);
    expect(libraryKinds('qoder')).toEqual(['skills', 'mcp']);
  });

  it('uses the same resource snapshot for Codex CLI and ChatGPT Desktop', async () => {
    expect(libraryKinds('chatgptdesktop')).toEqual(libraryKinds('codex'));
    expect(libraryKinds('claudedesktop')).toEqual(['skills', 'mcp', 'memory']);
    const refresh = deferredRead();
    vi.mocked(appLibrary.list)
      .mockResolvedValueOnce([resource('shared-skill')])
      .mockReturnValueOnce(refresh.promise);

    await act(async () => {
      renderer = create(
        <AppLibraryPanel key="codex" tool={tool('codex', 'Codex')} kind="skills" revision={0} />
      );
    });
    await act(async () => {
      renderer.update(
        <AppLibraryPanel
          key="chatgpt"
          tool={tool('chatgptdesktop', 'ChatGPT')}
          kind="skills"
          revision={0}
        />
      );
    });

    expect(JSON.stringify(renderer.toJSON())).toContain('shared-skill');
    expect(appLibrary.list).toHaveBeenLastCalledWith('chatgptdesktop', 'skills');
    await act(async () => refresh.resolve([]));
  });

  it('reuses Claude Code memory in Desktop without offering its shared store as a copy target', async () => {
    const desktop = tool('claudedesktop', 'Claude Desktop');
    useToolsStore.getState().setDetectedTools([claude, desktop, tool('codex', 'Codex')]);
    const refresh = deferredRead();
    vi.mocked(appLibrary.list)
      .mockResolvedValueOnce([{ ...resource('CLAUDE.md'), canCopy: true }])
      .mockReturnValueOnce(refresh.promise);

    await act(async () => {
      renderer = create(<AppLibraryPanel key="cli" tool={claude} kind="memory" revision={0} />);
    });
    await act(async () => {
      renderer.update(<AppLibraryPanel key="desktop" tool={desktop} kind="memory" revision={0} />);
    });

    expect(appLibrary.list).toHaveBeenLastCalledWith('claudedesktop', 'memory');
    expect(JSON.stringify(renderer.toJSON())).toContain('CLAUDE.md');
    act(() =>
      renderer.root.findByProps({ 'aria-label': 'appLibrary.copyTo · CLAUDE.md' }).props.onClick()
    );
    const dialog = renderer.root.findByType('dialog');
    expect(
      JSON.stringify(
        dialog.findAllByType('label').map((label) => label.findByType('span').children)
      )
    ).toBe(JSON.stringify([['Codex']]));
    await act(async () => refresh.resolve([]));
  });

  it.each([false, true])(
    'keeps the MCP add action and form above the list (populated: %s)',
    async (populated) => {
      const item = resource('fixture-server');
      vi.mocked(appLibrary.list).mockResolvedValue(populated ? [item] : []);
      await act(async () => {
        renderer = create(<AppLibraryPanel tool={claude} kind="mcp" revision={0} />);
      });

      const listText = populated ? item.name : 'appLibrary.empty';
      const closed = JSON.stringify(renderer.toJSON());
      expect(closed.indexOf('appLibrary.addMcp')).toBeGreaterThan(-1);
      expect(closed.indexOf('appLibrary.addMcp')).toBeLessThan(closed.indexOf(listText));

      act(() => renderer.root.findByType('button').props.onClick());
      const opened = JSON.stringify(renderer.toJSON());
      expect(opened.indexOf('appLibrary.serverConfig')).toBeGreaterThan(-1);
      expect(opened.indexOf('appLibrary.serverConfig')).toBeLessThan(opened.indexOf(listText));
    }
  );

  it.each([
    [true, 'appLibrary.enabledHint'],
    [false, 'appLibrary.disabledHint'],
    [null, 'appLibrary.enable'],
  ] as const)('describes resource actions and the switch state (%s)', async (enabled, hint) => {
    vi.mocked(appLibrary.list).mockResolvedValue([
      {
        ...resource('example'),
        enabled,
        canToggle: true,
        canCopy: true,
        canRemove: true,
        path: '/fixture',
      },
    ]);
    await act(async () => {
      renderer = create(<AppLibraryPanel tool={claude} kind="skills" revision={0} />);
    });

    const [toggle, copy, open, remove] = renderer.root.findAllByType('button');
    expect(toggle.props.title).toBe(hint);
    expect(toggle.props['aria-pressed']).toBe(enabled ?? undefined);
    expect(copy.props.title).toBe('appLibrary.copyTo');
    expect(open.props.title).toBe('appLibrary.openLocation');
    expect(remove.props.title).toBe('btn.delete');
  });

  it.each([
    [undefined, 'appLibrary.empty'],
    ['unsupported', 'appLibrary.readUnsupported'],
    ['invalidConfig', 'appLibrary.invalidConfig'],
    [new Error('IPC unavailable'), 'appLibrary.readFailed'],
  ])(
    'centers the panel notice when the resource list has no rows (%s)',
    async (failure, message) => {
      const list = vi.mocked(appLibrary.list);
      if (failure === undefined) list.mockResolvedValue([]);
      else list.mockRejectedValue(failure);
      await act(async () => {
        renderer = create(<AppLibraryPanel tool={claude} kind="mcp" revision={0} />);
      });

      const notice = renderer.root.findByType(AppPanelNotice).findByType('div');
      expect(notice.children).toEqual([message]);
      expect(notice.props.className).toContain('flex-1');
      expect(notice.props.className).toContain('justify-center');
      expect(notice.props.className).toContain('text-center');
      expect(notice.props.role).toBe(failure === undefined ? undefined : 'alert');
    }
  );

  it.each([true, false])(
    'keeps cached resources visible on a failed refresh (%s)',
    async (hasRows) => {
      vi.mocked(appLibrary.list)
        .mockResolvedValueOnce(hasRows ? [resource('cached-skill')] : [])
        .mockRejectedValueOnce('readFailed');
      await act(async () => {
        renderer = create(<AppLibraryPanel tool={claude} kind="skills" revision={0} />);
      });
      await act(async () => {
        renderer.update(<AppLibraryPanel tool={claude} kind="skills" revision={1} />);
      });

      const notice = renderer.root.findByType(AppPanelNotice).findByType('div');
      expect(notice.children).toEqual(['appLibrary.readFailed']);
      expect(notice.props.role).toBe(hasRows ? 'status' : 'alert');
      expect(notice.props.className.includes('flex-1')).toBe(!hasRows);
      const rendered = JSON.stringify(renderer.toJSON());
      expect(rendered.includes('cached-skill')).toBe(hasRows);
      expect(rendered).not.toContain('appLibrary.empty');
    }
  );

  it('keeps cached entries and empty states visible when revisiting a tab', async () => {
    const skillsRefresh = deferredRead();
    const pluginsRefresh = deferredRead();
    vi.mocked(appLibrary.list)
      .mockResolvedValueOnce([resource('cached-skill')])
      .mockResolvedValueOnce([])
      .mockReturnValueOnce(skillsRefresh.promise)
      .mockReturnValueOnce(pluginsRefresh.promise);

    await act(async () => {
      renderer = create(<AppLibraryPanel key="skills" tool={claude} kind="skills" revision={0} />);
    });
    await act(async () => {
      renderer.update(<AppLibraryPanel key="plugins" tool={claude} kind="plugins" revision={0} />);
    });
    expect(JSON.stringify(renderer.toJSON())).toContain('appLibrary.empty');

    await act(async () => {
      renderer.update(<AppLibraryPanel key="skills" tool={claude} kind="skills" revision={0} />);
    });
    expect(JSON.stringify(renderer.toJSON())).toContain('cached-skill');
    expect(renderer.root.findByProps({ role: 'tabpanel' }).props['aria-busy']).toBeUndefined();

    await act(async () => {
      renderer.update(<AppLibraryPanel key="plugins" tool={claude} kind="plugins" revision={0} />);
    });
    expect(JSON.stringify(renderer.toJSON())).toContain('appLibrary.empty');
    expect(renderer.root.findByProps({ role: 'tabpanel' }).props['aria-busy']).toBeUndefined();

    await act(async () => {
      skillsRefresh.resolve([resource('updated-skill')]);
      pluginsRefresh.resolve([]);
    });
    expect(useAppLibraryStore.getState().snapshots.claudecode?.skills?.entries?.[0].name).toBe(
      'updated-skill'
    );
  });

  it('shows a text-free skeleton on first load and discards older reads after refresh', async () => {
    vi.useFakeTimers();
    const oldRead = deferredRead();
    const freshRead = deferredRead();
    vi.mocked(appLibrary.list)
      .mockReturnValueOnce(oldRead.promise)
      .mockReturnValueOnce(freshRead.promise);
    await act(async () => {
      renderer = create(<AppLibraryPanel tool={claude} kind="skills" revision={0} />);
    });
    const panel = renderer.root.findByProps({ role: 'tabpanel' });
    expect(panel.props['aria-busy']).toBe(true);
    expect(panel.findAllByProps({ 'aria-hidden': 'true' })).toHaveLength(0);
    act(() => {
      vi.advanceTimersByTime(120);
    });
    expect(panel.findAllByProps({ 'aria-hidden': 'true' })).toHaveLength(1);
    expect(panel.findAllByProps({ role: 'status' })).toHaveLength(0);
    expect(JSON.stringify(renderer.toJSON())).not.toContain('appLibrary.empty');

    await act(async () => {
      renderer.update(<AppLibraryPanel tool={claude} kind="skills" revision={1} />);
    });
    await act(async () => freshRead.resolve([resource('fresh-skill')]));
    await act(async () => oldRead.resolve([resource('old-skill')]));
    expect(JSON.stringify(renderer.toJSON())).toContain('fresh-skill');
    expect(JSON.stringify(renderer.toJSON())).not.toContain('old-skill');
  });

  it('shows other apps only after asking to copy and writes on explicit confirmation', async () => {
    vi.mocked(appLibrary.list).mockResolvedValue([
      {
        id: '0/pdf',
        name: 'pdf',
        description: 'Read PDFs',
        group: null,
        path: '/fixture/pdf',
        enabled: null,
        canCopy: true,
        canRemove: true,
        canToggle: false,
      },
    ]);
    vi.mocked(appLibrary.copy).mockResolvedValue([{ toolId: 'codex', status: 'copied' }]);
    await act(async () => {
      renderer = create(<AppLibraryPanel tool={claude} kind="skills" revision={0} />);
    });
    expect(appLibrary.list).toHaveBeenCalledExactlyOnceWith('claudecode', 'skills');
    expect(JSON.stringify(renderer.toJSON())).not.toContain('Codex');
    act(() =>
      renderer.root.findByProps({ 'aria-label': 'appLibrary.copyTo · pdf' }).props.onClick()
    );
    const dialog = renderer.root.findByType('dialog');
    expect(JSON.stringify(renderer.toJSON())).toContain('Codex');
    expect(JSON.stringify(renderer.toJSON())).not.toContain('Cursor');
    expect(JSON.stringify(renderer.toJSON())).not.toContain('ChatGPT');
    expect(appLibrary.copy).not.toHaveBeenCalled();
    act(() =>
      dialog.findByProps({ type: 'checkbox' }).props.onChange({ target: { checked: true } })
    );
    const [cancel, copy] = dialog.findAllByType('button');
    expect(cancel.children).toContain('btn.cancel');
    await act(async () => copy.props.onClick());
    expect(appLibrary.copy).toHaveBeenCalledExactlyOnceWith('claudecode', 'skills', '0/pdf', [
      'codex',
    ]);
    expect(JSON.stringify(renderer.toJSON())).toContain('appLibrary.copied');
  });
});
