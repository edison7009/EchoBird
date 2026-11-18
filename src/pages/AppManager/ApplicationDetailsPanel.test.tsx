import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import type { LocalTool } from '../../api/types';
import {
  ApplicationDetailsPanel,
  ApplicationActions,
  useApplicationDetails,
} from './ApplicationDetailsPanel';
import { useSystemApplicationsStore } from '../../stores/systemApplicationsStore';
import { useNavigationStore } from '../../stores/navigationStore';

vi.mock('../../api/tauri', () => ({
  getSystemApplicationDetails: vi.fn(),
  getToolConfigurationPaths: vi.fn(),
  openFolder: vi.fn(),
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    locale: 'en',
    t: (key: string) => (key === 'mother.hintUninstall' ? 'Completely uninstall {agent}' : key),
  }),
}));

let tool: LocalTool = {
  id: 'system:one',
  systemAppId: 'system:one',
  name: 'Local app',
  category: 'Desktop',
  installed: true,
};
const details: api.SystemApplicationDetails = {
  path: '/opt/Local app/start',
  directory: '/opt/Local app',
  fileSize: 2048,
  modifiedAt: 1700000000000,
  version: '1.2.3',
  launchTarget: '/share/applications/local.desktop',
};
let renderer: ReactTestRenderer;
beforeEach(() => {
  tool = { ...tool };
  vi.clearAllMocks();
  vi.mocked(api.getToolConfigurationPaths).mockReset().mockResolvedValue([]);
  useSystemApplicationsStore.setState(useSystemApplicationsStore.getInitialState(), true);
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
  vi.mocked(api.getSystemApplicationDetails).mockResolvedValue(details);
  vi.mocked(api.openFolder).mockResolvedValue(undefined);
});
afterEach(() => {
  act(() => renderer?.unmount());
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
});
const text = () => JSON.stringify(renderer.toJSON());
function Details({ tool }: { tool: LocalTool }) {
  const state = useApplicationDetails(tool);
  return (
    <ApplicationDetailsPanel tool={tool} state={state}>
      <ApplicationActions
        tool={tool}
        directory={state.directory}
        path={state.data?.path}
        launchTarget={state.data?.launchTarget}
        configurationPaths={state.configurationPaths}
        disabled={state.loading}
      />
    </ApplicationDetailsPanel>
  );
}
const locate = () =>
  renderer.root
    .findAllByType('button')
    .find((button) => button.children.includes('aiDesktop.openLocation'))!;

it('reads only the selected application and opens its directory only on explicit click', async () => {
  await act(async () => {
    renderer = create(<Details tool={tool} />);
  });
  expect(api.getSystemApplicationDetails).toHaveBeenCalledExactlyOnceWith('system:one');
  expect(api.openFolder).not.toHaveBeenCalled();
  expect(text()).toContain('2 KiB');
  expect(text()).toContain(details.path);
  expect(text()).toContain(details.launchTarget);
  expect(text()).toContain('1.2.3');
  expect(renderer.root.findByType(ApplicationActions).findAllByType('svg')).toHaveLength(0);
  expect(renderer.root.findByType(ApplicationActions).findAllByType('button')).toHaveLength(3);
  await act(async () => locate().props.onClick());
  expect(api.openFolder).toHaveBeenCalledExactlyOnceWith('/opt/Local app');
});

it('shows pending attributes and ignores late responses after changing apps', async () => {
  let complete!: (data: api.SystemApplicationDetails) => void;
  vi.mocked(api.getSystemApplicationDetails)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        })
    )
    .mockResolvedValueOnce({ ...details, path: '/new/start', fileSize: 0 });
  act(() => {
    renderer = create(<Details tool={tool} />);
  });
  expect(renderer.root.findByType('section').props['aria-busy']).toBe(true);
  expect(renderer.root.findAllByType('button').every((button) => button.props.disabled)).toBe(true);
  await act(async () =>
    renderer.update(<Details tool={{ ...tool, id: 'system:two', systemAppId: 'system:two' }} />)
  );
  await act(async () => complete(details));
  expect(text()).toContain('/new/start');
  expect(text()).toContain('0 B');
  expect(text()).not.toContain('/opt/Local app/start');
  act(() =>
    renderer.root
      .findAllByType('button')
      .find((button) => button.children.includes('aiDesktop.thoroughUninstall'))!
      .props.onClick()
  );
  expect(useNavigationStore.getState().motherPrefill).toContain('/new/start');
  expect(useNavigationStore.getState().motherPrefill).not.toContain('/opt/Local app/start');
});

it('keeps missing attributes unknown and reports read failures inline', async () => {
  vi.mocked(api.getSystemApplicationDetails).mockRejectedValue(new Error('Unavailable'));
  await act(async () => {
    renderer = create(<Details tool={tool} />);
  });
  expect(renderer.root.findByProps({ role: 'status' }).children).toContain('Error: Unavailable');
  expect(locate().props.disabled).toBe(true);
  expect(text()).not.toContain('0 B');
  expect(api.openFolder).not.toHaveBeenCalled();
});

it('refreshes attributes when a new inventory snapshot replaces the same application', async () => {
  vi.mocked(api.getSystemApplicationDetails)
    .mockResolvedValueOnce(details)
    .mockResolvedValueOnce({ ...details, version: '2.0' });
  await act(async () => {
    renderer = create(<Details tool={tool} />);
  });
  await act(async () => renderer.update(<Details tool={{ ...tool }} />));
  expect(api.getSystemApplicationDetails).toHaveBeenCalledTimes(2);
  expect(text()).toContain('2.0');
});

it('returns cached attributes immediately when reselecting an application', async () => {
  const other = { ...tool, id: 'system:other', systemAppId: 'system:other' };
  vi.mocked(api.getSystemApplicationDetails)
    .mockResolvedValueOnce(details)
    .mockResolvedValueOnce({ ...details, path: '/other/start' });
  await act(async () => {
    renderer = create(<Details tool={tool} />);
  });
  await act(async () => renderer.update(<Details tool={other} />));
  act(() => {
    renderer.update(<Details tool={tool} />);
  });
  expect(renderer.root.findByType('section').props['aria-busy']).toBeUndefined();
  expect(text()).toContain(details.path);
  expect(locate().props.disabled).toBe(false);
  expect(api.getSystemApplicationDetails).toHaveBeenCalledTimes(2);
  expect(api.openFolder).not.toHaveBeenCalled();
});

it('shares pending reads and caches late results without showing the previous app', async () => {
  const other = { ...tool, id: 'system:other', systemAppId: 'system:other' };
  let completeFirst!: (data: api.SystemApplicationDetails) => void;
  let completeOther!: (data: api.SystemApplicationDetails) => void;
  vi.mocked(api.getSystemApplicationDetails)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          completeFirst = resolve;
        })
    )
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          completeOther = resolve;
        })
    );
  act(() => {
    renderer = create(<Details tool={tool} />);
  });
  act(() => renderer.update(<Details tool={other} />));
  act(() => renderer.update(<Details tool={tool} />));
  expect(api.getSystemApplicationDetails).toHaveBeenCalledTimes(2);
  await act(async () => completeOther({ ...details, path: '/other/start' }));
  expect(text()).not.toContain('/other/start');
  expect(renderer.root.findByType('section').props['aria-busy']).toBe(true);
  await act(async () => completeFirst(details));
  expect(text()).toContain(details.path);
  act(() => renderer.update(<Details tool={other} />));
  expect(renderer.root.findByType('section').props['aria-busy']).toBeUndefined();
  expect(text()).toContain('/other/start');
  expect(api.getSystemApplicationDetails).toHaveBeenCalledTimes(2);
});

it('retries failures on re-entry and disables actions while that retry is pending', async () => {
  let complete!: (data: api.SystemApplicationDetails) => void;
  vi.mocked(api.getSystemApplicationDetails)
    .mockRejectedValueOnce(new Error('Unavailable'))
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        })
    );
  await act(async () => {
    renderer = create(<Details tool={tool} />);
  });
  expect(text()).toContain('Unavailable');
  act(() => renderer.update(<Details tool={{ ...tool, systemAppId: undefined }} />));
  act(() => renderer.update(<Details tool={tool} />));
  expect(text()).not.toContain('Unavailable');
  expect(renderer.root.findByType('section').props['aria-busy']).toBe(true);
  expect(renderer.root.findAllByType('button').every((button) => button.props.disabled)).toBe(true);
  await act(async () => complete(details));
  expect(text()).toContain(details.path);
});

it('keeps the same property rows through loading, missing and long values', async () => {
  let complete!: (data: api.SystemApplicationDetails) => void;
  vi.mocked(api.getSystemApplicationDetails).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      })
  );
  act(() => {
    renderer = create(<Details tool={tool} />);
  });
  const rows = () => renderer.root.findAllByType('dt').map((node) => node.children);
  const heights = () => renderer.root.findAllByType('dd').map((node) => node.props.className);
  const expectedRows = rows();
  const expectedHeights = heights();
  expect(expectedRows).toHaveLength(5);
  await act(async () =>
    complete({ ...details, path: `/opt/${'very-long-path/'.repeat(30)}start` })
  );
  expect(rows()).toEqual(expectedRows);
  expect(heights()).toEqual(expectedHeights);
  const other = { ...tool, id: 'system:missing', systemAppId: 'system:missing' };
  vi.mocked(api.getSystemApplicationDetails).mockResolvedValueOnce({
    ...details,
    path: null,
    directory: null,
    version: null,
    fileSize: null,
    modifiedAt: null,
  });
  await act(async () => renderer.update(<Details tool={other} />));
  expect(rows()).toEqual(expectedRows);
  expect(heights()).toEqual(expectedHeights);
  expect(text()).toContain('—');
});

it('shows folder failures without losing loaded attributes', async () => {
  vi.mocked(api.openFolder).mockRejectedValue(new Error('Missing folder'));
  await act(async () => {
    renderer = create(<Details tool={tool} />);
  });
  await act(async () => locate().props.onClick());
  expect(renderer.root.findByProps({ role: 'status' }).children).toContain('Error: Missing folder');
  expect(text()).toContain('2 KiB');
});

it('uses existing managed tool attributes without reading system application data', () => {
  act(() => {
    renderer = create(
      <Details
        tool={{ ...tool, systemAppId: undefined, detectedPath: '/managed/app', version: '2.0' }}
      />
    );
  });
  expect(api.getSystemApplicationDetails).not.toHaveBeenCalled();
  expect(text()).toContain('/managed/app');
  expect(text()).toContain('2.0');
});

it.each([
  ['C:\\App.exe', 'C:\\'],
  ['/App', '/'],
])('locates managed root-level files safely for %s', async (path, directory) => {
  act(() => {
    renderer = create(<Details tool={{ ...tool, systemAppId: undefined, detectedPath: path }} />);
  });
  await act(async () => locate().props.onClick());
  expect(api.openFolder).toHaveBeenCalledExactlyOnceWith(directory);
});

it.each([{ installed: false }, { id: 'claudeduet' }])(
  'keeps the right-side uninstall action hidden for excluded entries %j',
  (overrides) => {
    act(() => {
      renderer = create(<ApplicationActions tool={{ ...tool, ...overrides }} disabled={false} />);
    });
    expect(renderer.toJSON()).toBeNull();
  }
);

it('disables the right-side uninstall action while launching', () => {
  act(() => {
    renderer = create(<ApplicationActions tool={tool} disabled />);
  });
  expect(renderer.root.findAllByType('button').every((button) => button.props.disabled)).toBe(true);
});

it.each([
  'D:\\Custom Apps $&\\App.exe',
  '/Applications/Custom App.app',
  '/opt/custom app/bin/start',
])('includes literal native paths in the AI request for %s', (path) => {
  act(() => {
    renderer = create(
      <ApplicationActions
        tool={{ ...tool, name: 'App $& $`' }}
        path={path}
        launchTarget={path}
        disabled={false}
      />
    );
  });
  act(() => renderer.root.findAllByType('button')[1].props.onClick());
  expect(useNavigationStore.getState().motherPrefill).toBe(
    `Completely uninstall App $& $\`\naiDesktop.fileLocation ${path}`
  );
});

it('looks up managed configuration only on uninstall click and retains its detected path', async () => {
  const path = 'D:\\Managed App\\App.exe';
  act(() => {
    renderer = create(
      <ApplicationActions
        tool={{ ...tool, systemAppId: undefined, detectedPath: path }}
        disabled={false}
      />
    );
  });
  expect(api.getToolConfigurationPaths).not.toHaveBeenCalled();
  await act(async () => renderer.root.findAllByType('button')[1].props.onClick());
  expect(api.getToolConfigurationPaths).toHaveBeenCalledExactlyOnceWith(tool.id);
  expect(useNavigationStore.getState().motherPrefill).toContain(path);
  expect(api.getSystemApplicationDetails).not.toHaveBeenCalled();
  expect(api.openFolder).not.toHaveBeenCalled();
});

it('keeps the registered launch entry when a filesystem path is unavailable', () => {
  const target = 'shell:AppsFolder\\Vendor.App_id!App';
  act(() => {
    renderer = create(
      <ApplicationActions tool={{ ...tool, detectedPath: target }} disabled={false} />
    );
  });
  act(() => renderer.root.findAllByType('button')[1].props.onClick());
  expect(useNavigationStore.getState().motherPrefill).toBe(
    `Completely uninstall Local app\naiDesktop.launchTarget ${target}`
  );
});

it('includes Store package location and its distinct activation entry', () => {
  const path = 'C:\\Program Files\\WindowsApps\\Vendor.App_1.0_x64_id';
  const target = 'shell:AppsFolder\\Vendor.App_id!App';
  act(() => {
    renderer = create(
      <ApplicationActions tool={tool} path={path} launchTarget={target} disabled={false} />
    );
  });
  act(() => renderer.root.findAllByType('button')[1].props.onClick());
  expect(useNavigationStore.getState().motherPrefill).toBe(
    `Completely uninstall Local app\naiDesktop.fileLocation ${path}\naiDesktop.launchTarget ${target}`
  );
});

it.each(['all', 'desktop'] as const)(
  'toggles My visibility below the actions while retaining system details (%s)',
  async (tab) => {
    useNavigationStore.getState().setAppsTab(tab);
    await act(async () => {
      renderer = create(<Details tool={tool} />);
    });
    const toggle = () =>
      renderer.root
        .findAllByType('button')
        .find(
          (button) =>
            button.children.includes('aiDesktop.addToMy') ||
            button.children.includes('aiDesktop.removeFromMy')
        )!;
    const markup = text();
    expect(markup.indexOf('aiDesktop.addToMy')).toBeGreaterThan(
      markup.indexOf('aiDesktop.thoroughUninstall')
    );
    expect(toggle().props['aria-pressed']).toBe(false);
    act(() => toggle().props.onClick());
    expect(useSystemApplicationsStore.getState().myApplicationIds).toEqual([tool.systemAppId]);
    expect(toggle().children).toContain('aiDesktop.removeFromMy');
    expect(text()).toContain(details.path);
    expect(text()).toContain(details.version);
    expect(api.getSystemApplicationDetails).toHaveBeenCalledOnce();
    expect(api.openFolder).not.toHaveBeenCalled();
    expect(useNavigationStore.getState().activePage).toBe('apps');
    act(() => toggle().props.onClick());
    expect(useSystemApplicationsStore.getState().myApplicationIds).toEqual([]);
    expect(toggle().children).toContain('aiDesktop.addToMy');
  }
);

it('keeps fixed My entries free of add/remove controls in the system panel', async () => {
  const original = { ...tool, defaultInMyApplications: true };
  await act(async () => {
    renderer = create(<Details tool={original} />);
  });
  const actions = renderer.root.findByType(ApplicationActions).findAllByType('button');
  expect(actions.map((button) => button.children)).toEqual([
    ['aiDesktop.openLocation'],
    ['aiDesktop.thoroughUninstall'],
  ]);
  expect(useSystemApplicationsStore.getState().myApplicationIds).toEqual([]);
});

it('shows found configuration/data locations and sends literal paths as clues for thorough uninstall', async () => {
  const first = '/Users/fixture/Library/Containers/com.example.App';
  const second = '/Users/fixture/Library/Preferences/com.example.App.plist';
  vi.mocked(api.getSystemApplicationDetails).mockResolvedValueOnce({
    ...details,
    configPaths: [first, second, first],
  });
  await act(async () => {
    renderer = create(<Details tool={tool} />);
  });
  expect(text()).toContain('aiDesktop.configurationLocations');
  expect(text()).toContain(first);
  expect(text()).toContain(second);
  expect(renderer.root.findByType(ApplicationActions).props.configurationPaths).toEqual([
    first,
    second,
  ]);
  act(() =>
    renderer.root
      .findAllByType('button')
      .find((button) => button.children.includes('aiDesktop.thoroughUninstall'))!
      .props.onClick()
  );
  const prefill = useNavigationStore.getState().motherPrefill!;
  expect(prefill).toContain(JSON.stringify(first));
  expect(prefill).toContain(JSON.stringify(second));
  expect(prefill.split(JSON.stringify(first))).toHaveLength(2);
  expect(prefill).toContain('aiDesktop.configurationLocationsHint');
  expect(api.openFolder).not.toHaveBeenCalled();
});

it('deduplicates managed configuration clues without sending or changing the client', async () => {
  const config = '/home/fixture/.app/settings.json';
  vi.mocked(api.getToolConfigurationPaths).mockResolvedValueOnce([config, config]);
  act(() => {
    renderer = create(
      <ApplicationActions tool={{ ...tool, systemAppId: undefined }} disabled={false} />
    );
  });
  expect(api.getToolConfigurationPaths).not.toHaveBeenCalled();
  await act(async () => renderer.root.findAllByType('button')[1].props.onClick());
  const prefill = useNavigationStore.getState().motherPrefill!;
  expect(prefill.split(JSON.stringify(config))).toHaveLength(2);
  expect(prefill).toContain('aiDesktop.configurationLocationsHint');
  expect(api.openFolder).not.toHaveBeenCalled();
});

it('guards repeated clicks and ignores managed paths returned after selecting another app', async () => {
  let finish!: (paths: string[]) => void;
  vi.mocked(api.getToolConfigurationPaths).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  const managed = { ...tool, systemAppId: undefined };
  act(() => {
    renderer = create(<ApplicationActions tool={managed} disabled={false} />);
  });
  act(() => {
    renderer.root.findAllByType('button')[1].props.onClick();
    renderer.root.findAllByType('button')[1].props.onClick();
  });
  expect(api.getToolConfigurationPaths).toHaveBeenCalledOnce();
  expect(renderer.root.findAllByType('button')[1].props.disabled).toBe(false);
  act(() =>
    renderer.update(<ApplicationActions tool={{ ...managed, id: 'other' }} disabled={false} />)
  );
  await act(async () => finish(['/old/config']));
  expect(useNavigationStore.getState().motherPrefill).toBeUndefined();
});

it('does not reopen the assistant when managed path lookup finishes after leaving apps', async () => {
  let finish!: (paths: string[]) => void;
  vi.mocked(api.getToolConfigurationPaths).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    })
  );
  act(() => {
    renderer = create(
      <ApplicationActions tool={{ ...tool, systemAppId: undefined }} disabled={false} />
    );
  });
  act(() => {
    renderer.root.findAllByType('button')[1].props.onClick();
  });
  act(() => useNavigationStore.getState().setActivePage('models'));
  await act(async () => finish(['/app/config']));
  expect(useNavigationStore.getState().activePage).toBe('models');
  expect(useNavigationStore.getState().motherPrefill).toBeUndefined();
});

it('still prefills the managed uninstall request if configuration discovery fails', async () => {
  vi.mocked(api.getToolConfigurationPaths).mockRejectedValueOnce(new Error('fixture failure'));
  act(() => {
    renderer = create(
      <ApplicationActions tool={{ ...tool, systemAppId: undefined }} disabled={false} />
    );
  });
  await act(async () => renderer.root.findAllByType('button')[1].props.onClick());
  expect(useNavigationStore.getState().motherPrefill).toContain('Completely uninstall');
  expect(useNavigationStore.getState().motherPrefill).not.toContain(
    'aiDesktop.configurationLocations'
  );
});
