import React from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, expect, it, vi } from 'vitest';
import { AppManagerContext, type AppManagerContextType } from './context';
import { AppManagerBottom, AppManagerPanel } from './AppManagerComponents';
import { useNavigationStore } from '../../stores/navigationStore';
import { ApplicationActions, ApplicationDetailsPanel } from './ApplicationDetailsPanel';
import * as api from '../../api/tauri';
import type { LocalTool } from '../../api/types';

vi.mock('../../components', () => ({ getModelIcon: () => null }));
vi.mock('../ModelNexus/context', () => ({ useModelNexus: () => ({}) }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => vi.fn() }));
vi.mock('../../api/tauri', () => ({
  getSystemApplicationDetails: vi.fn().mockResolvedValue({
    path: 'D:\\Apps $&\\Local App.exe',
    directory: 'D:\\Apps $&',
    launchTarget: 'shell:AppsFolder\\fixture',
    fileSize: null,
    modifiedAt: null,
    version: null,
  }),
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    locale: 'zh-Hans',
    t: (key: string) =>
      (
        ({
          'btn.uninstall': '卸载',
          'btn.launchApp': '启动应用',
          'mother.hintUninstall': '彻底卸载 {agent}',
          'aiDesktop.openLocation': '打开文件夹',
          'aiDesktop.fileLocation': '文件位置',
          'aiDesktop.launchTarget': '启动入口',
        }) as Record<string, string>
      )[key] || key,
  }),
}));

let renderer: ReactTestRenderer;
afterEach(() => {
  act(() => renderer?.unmount());
  useNavigationStore.setState(useNavigationStore.getInitialState(), true);
});

it.each(['本机应用', 'App $& $`'])(
  'opens the existing AI message flow with the literal application name %s',
  async (name) => {
    useNavigationStore.getState().setAppsTab('all');
    const handleLaunch = vi.fn();
    const context = {
      viewMode: 'all',
      selectedTool: 'system:one',
      selectedToolData: {
        id: 'system:one',
        systemAppId: 'system:one',
        name,
        category: 'Desktop',
        installed: true,
        noModelConfig: true,
      },
      toolModelConfig: {},
      userModels: [],
      isLaunching: false,
      handleLaunch,
    } as unknown as AppManagerContextType;
    await act(async () => {
      renderer = create(
        <AppManagerContext.Provider value={context}>
          <AppManagerBottom />
          <AppManagerPanel />
        </AppManagerContext.Provider>
      );
    });
    const primary = renderer.root.findByType(AppManagerBottom).findAllByType('button');
    expect(primary.map((action) => action.props.children)).toEqual(['启动应用']);
    expect(primary[0].parent?.props.className).toContain('w-64');
    const uninstall = renderer.root
      .findByType(AppManagerPanel)
      .findAllByType('button')
      .find((action) => action.children.includes('卸载'))!;
    expect(handleLaunch).not.toHaveBeenCalled();
    const detailsPanel = renderer.root.findByType(ApplicationDetailsPanel);
    expect(detailsPanel.findAllByType(ApplicationActions)).toHaveLength(1);
    const actionContainer = detailsPanel.findByType(ApplicationActions).findAllByType('div')[0];
    expect(actionContainer.props.className).not.toContain('border-t');
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup.indexOf('打开文件夹')).toBeLessThan(markup.indexOf('文件位置'));
    expect(uninstall.props.disabled).toBe(false);
    act(() => primary[0].props.onClick());
    expect(handleLaunch).toHaveBeenCalledOnce();
    act(() => uninstall.props.onClick());
    expect(useNavigationStore.getState()).toMatchObject({
      activePage: 'mother',
      motherPrefill: `彻底卸载 ${name}\n文件位置 D:\\Apps $&\\Local App.exe\n启动入口 shell:AppsFolder\\fixture`,
    });
    expect(
      renderer.root.findAllByType('button').map((action) => action.props.children)
    ).not.toContain(`彻底卸载 ${name}`);
  }
);

function renderPanel(tool: LocalTool) {
  const context = {
    selectedTool: tool.id,
    selectedToolData: tool,
    userModels: [],
    toolModelConfig: {},
    isLaunching: false,
    viewMode: 'all',
  } as unknown as AppManagerContextType;
  return (
    <AppManagerContext.Provider value={context}>
      <AppManagerPanel />
    </AppManagerContext.Provider>
  );
}
const nativeTool = (id: string): LocalTool => ({
  id,
  systemAppId: id,
  name: id,
  category: 'Desktop',
  installed: true,
});
const nativeDetails = (id: string): api.SystemApplicationDetails => ({
  path: `/apps/${id}/start`,
  directory: `/apps/${id}`,
  launchTarget: `shell:${id}`,
  version: id,
  fileSize: 2048,
  modifiedAt: 1700000000000,
});

it('keeps the same action buttons and labels without opacity or value skeleton changes on first load', async () => {
  let complete!: (data: api.SystemApplicationDetails) => void;
  vi.mocked(api.getSystemApplicationDetails)
    .mockReset()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        })
    );
  act(() => {
    renderer = create(renderPanel(nativeTool('first')));
  });
  const buttons = renderer.root.findByType(ApplicationActions).findAllByType('button');
  const labels = renderer.root.findAllByType('dt');
  expect(buttons).toHaveLength(2);
  expect(buttons.every((button) => button.props.disabled)).toBe(true);
  const classes = buttons.map((button) => button.props.className);
  expect(classes.every((className) => !className.includes('opacity'))).toBe(true);
  expect(renderer.root.findByType('dl').props.className).not.toContain('animate');
  expect(renderer.root.findByType('dl').findAllByType('span')).toHaveLength(0);
  await act(async () => complete(nativeDetails('first')));
  expect(renderer.root.findByType(ApplicationActions).findAllByType('button')).toEqual(buttons);
  expect(renderer.root.findAllByType('dt')).toEqual(labels);
  expect(buttons.map((button) => button.props.className)).toEqual(classes);
  expect(buttons.every((button) => !button.props.disabled)).toBe(true);
});

it('replaces one coherent detail panel only after the next app is ready and ignores intermediate responses', async () => {
  const first = nativeTool('first');
  const second = nativeTool('second');
  const third = nativeTool('third');
  let completeSecond!: (data: api.SystemApplicationDetails) => void;
  let completeThird!: (data: api.SystemApplicationDetails) => void;
  vi.mocked(api.getSystemApplicationDetails)
    .mockReset()
    .mockResolvedValueOnce(nativeDetails('first'))
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          completeSecond = resolve;
        })
    )
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          completeThird = resolve;
        })
    );
  await act(async () => {
    renderer = create(renderPanel(first));
  });
  const buttons = renderer.root.findByType(ApplicationActions).findAllByType('button');
  const values = () => renderer.root.findAllByType('dd').map((node) => node.children);
  const originalValues = values();
  act(() => renderer.update(renderPanel(second)));
  expect(renderer.root.findByType(ApplicationDetailsPanel).props.tool).toBe(first);
  expect(values()).toEqual(originalValues);
  expect(buttons.every((button) => button.props.disabled)).toBe(true);
  expect(renderer.root.findByType('dl').findAllByType('span')).toHaveLength(0);
  act(() => renderer.update(renderPanel(third)));
  await act(async () => completeSecond(nativeDetails('second')));
  expect(renderer.root.findByType(ApplicationDetailsPanel).props.tool).toBe(first);
  expect(values()).toEqual(originalValues);
  await act(async () => completeThird(nativeDetails('third')));
  expect(renderer.root.findByType(ApplicationDetailsPanel).props.tool).toBe(third);
  expect(JSON.stringify(values())).toContain('/apps/third/start');
  expect(JSON.stringify(values())).not.toContain('/apps/first/start');
  expect(renderer.root.findByType(ApplicationActions).findAllByType('button')).toEqual(buttons);
  expect(buttons.every((button) => !button.props.disabled)).toBe(true);
  expect(useNavigationStore.getState().activePage).toBe('apps');
});

it('attributes a failed new read to the selected app and releases the previous detail values', async () => {
  const first = nativeTool('first');
  const second = nativeTool('second');
  let fail!: (error: Error) => void;
  vi.mocked(api.getSystemApplicationDetails)
    .mockReset()
    .mockResolvedValueOnce(nativeDetails('first'))
    .mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          fail = reject;
        })
    );
  await act(async () => {
    renderer = create(renderPanel(first));
  });
  act(() => renderer.update(renderPanel(second)));
  expect(renderer.root.findByType(ApplicationDetailsPanel).props.tool).toBe(first);
  await act(async () => fail(new Error('Unavailable')));
  expect(renderer.root.findByType(ApplicationDetailsPanel).props.tool).toBe(second);
  expect(renderer.root.findByProps({ role: 'status' }).children).toContain('Error: Unavailable');
  expect(JSON.stringify(renderer.toJSON())).not.toContain('/apps/first/start');
});
