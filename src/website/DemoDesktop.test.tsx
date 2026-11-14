import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, expect, it, vi } from 'vitest';
import { DemoDesktop } from './DemoDesktop';
import { I18nContext } from '../hooks/i18nContext';
const demo = (locale: 'zh-Hans' | 'en' = 'zh-Hans') => (
  <I18nContext.Provider value={{ locale, setLocale: () => {}, t: (key) => key }}>
    <DemoDesktop />
  </I18nContext.Provider>
);
import { AccountSectionRow } from '../pages/AppManager/AccountSectionPrimitives';
import { ModelListCard } from '../components/ModelListCard';
import { DesktopIcon } from '../components/DesktopToolIcon';
import { ViewModeTabs } from '../components/ViewModeTabs';
import { PageActionButton } from '../components/PageActionButton';
import * as api from '../api/tauri';

let tree: ReactTestRenderer | undefined;
afterEach(() => {
  if (tree) act(() => tree!.unmount());
  tree = undefined;
  vi.restoreAllMocks();
});

it('keeps tool, tab, account and model selection in memory without native operations', () => {
  const scan = vi.spyOn(api, 'scanTools');
  const settings = vi.spyOn(api, 'saveSettings');
  const apply = vi.spyOn(api, 'applyModelToTool');
  act(() => {
    tree = create(demo());
  });
  const root = tree!.root;
  expect(root.findAllByType(DesktopIcon)).toHaveLength(14);
  const toolIds = root.findAllByType(DesktopIcon).map((icon) => icon.props.tool.id);
  expect(toolIds).not.toContain('qwenwork');
  expect(toolIds).not.toContain('traecn');
  const initialStatus = root.findByProps({ role: 'status' }).children.join('');
  act(() => root.findAllByType(AccountSectionRow)[0].props.onSelect());
  expect(root.findAllByType(ModelListCard).every((card) => !card.props.selected)).toBe(true);
  act(() => root.findAllByType(ModelListCard)[1].props.onSelect());
  expect(root.findAllByType(AccountSectionRow).every((row) => !row.props.selected)).toBe(true);
  act(() => root.findAllByType(DesktopIcon)[1].props.onClick());
  expect(root.findAllByType(ModelListCard).every((card) => !card.props.selected)).toBe(true);
  act(() => root.findByType(ViewModeTabs).props.onChange('install'));
  expect(root.findAllByType('button').some((button) => button.children.includes('应用启动'))).toBe(
    true
  );
  expect(root.findByProps({ role: 'status' }).children.join('')).toBe(initialStatus);
  expect(scan).not.toHaveBeenCalled();
  expect(settings).not.toHaveBeenCalled();
  expect(apply).not.toHaveBeenCalled();
});

it('requires confirmation before deleting an example account', () => {
  act(() => {
    tree = create(demo());
  });
  const root = tree!.root;
  act(() => root.findAllByType(AccountSectionRow)[0].props.onDelete());
  expect(root.findAllByProps({ role: 'dialog' })).toHaveLength(1);
  expect(root.findAllByType(AccountSectionRow)).toHaveLength(2);
  const cancel = root
    .findAllByType(PageActionButton)
    .find((button) => button.props.children === '取消')!;
  act(() => cancel.props.onClick());
  expect(root.findAllByType(AccountSectionRow)).toHaveLength(2);
  act(() => root.findAllByType(AccountSectionRow)[0].props.onDelete());
  const confirm = root
    .findAllByType(PageActionButton)
    .find((button) => button.props.children === '删除')!;
  act(() => confirm.props.onClick());
  expect(root.findAllByType(AccountSectionRow)).toHaveLength(1);
});

it('shows native model actions and isolates them from selection and real quota queries', () => {
  const query = vi.spyOn(api, 'queryModelUsage');
  act(() => {
    tree = create(demo());
  });
  const root = tree!.root;
  const card = root
    .findAllByType(ModelListCard)
    .find((item) => item.props.model.name === 'DeepSeek')!;
  expect(card.props.usage.quotas[0].balance).toBe(13.92);
  const stopPropagation = vi.fn();
  const refresh = card
    .findAllByType('button')
    .find((button) => button.props['aria-label'] === 'btn.refreshUsage')!;
  const edit = card
    .findAllByType('button')
    .find((button) => button.props['aria-label'] === 'btn.edit')!;
  const remove = card
    .findAllByType('button')
    .find((button) => button.props['aria-label'] === 'btn.delete')!;
  act(() => refresh.props.onClick({ stopPropagation }));
  act(() => edit.props.onClick({ stopPropagation }));
  expect(card.props.selected).toBe(false);
  act(() => remove.props.onClick({ stopPropagation }));
  expect(root.findAllByProps({ role: 'dialog' })).toHaveLength(1);
  expect(root.findAllByType(ModelListCard)).toHaveLength(4);
  const confirm = root
    .findAllByType(PageActionButton)
    .find((button) => button.props.children === '删除')!;
  act(() => confirm.props.onClick());
  expect(root.findAllByType(ModelListCard)).toHaveLength(3);
  expect(stopPropagation).toHaveBeenCalledTimes(3);
  expect(query).not.toHaveBeenCalled();
});

it('uses independent capability pills while keeping selection and footer copy unchanged', () => {
  act(() => {
    tree = create(demo());
  });
  const root = tree!.root;
  const selected = root.findAllByType(ModelListCard).map((card) => card.props.selected);
  const status = root.findByProps({ role: 'status' }).children.join('');
  expect(root.findByProps({ 'aria-label': 'Web Search' }).props['aria-pressed']).toBe(true);
  expect(root.findByProps({ 'aria-label': '1M' }).props['aria-pressed']).toBe(false);
  act(() => root.findByProps({ 'aria-label': '1M' }).props.onClick());
  expect(root.findByProps({ 'aria-label': '1M' }).props['aria-pressed']).toBe(true);
  expect(root.findByProps({ 'aria-label': 'Web Search' }).props['aria-pressed']).toBe(true);
  act(() => root.findByProps({ 'aria-label': 'Web Search' }).props.onClick());
  expect(root.findByProps({ 'aria-label': 'Web Search' }).props['aria-pressed']).toBe(false);
  expect(root.findByProps({ 'aria-label': '1M' }).props['aria-pressed']).toBe(true);
  expect(root.findAllByType(ModelListCard).map((card) => card.props.selected)).toEqual(selected);
  expect(root.findByProps({ role: 'status' }).children.join('')).toBe(status);
});

it('renders the English demo without untranslated Chinese labels', () => {
  act(() => {
    tree = create(demo('en'));
  });
  expect(tree!.root.findByProps({ role: 'status' }).children.join('')).toBe(
    'Interactive demo · Sample data'
  );
  expect(JSON.stringify(tree!.toJSON())).not.toMatch(/[\u3400-\u9fff]/);
});
