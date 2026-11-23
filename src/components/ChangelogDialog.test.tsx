import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { useLayoutEffect, useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { openExternal } from '../api/tauri';
import { ChangelogDialog } from './ChangelogDialog';
import { PageActionButton } from './PageActionButton';
import simplifiedChinese from '../i18n/zh-Hans';

vi.mock('../api/tauri', () => ({ openExternal: vi.fn() }));

const language = vi.hoisted(() => ({ locale: 'en' }));
vi.mock('../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: keyof typeof simplifiedChinese) =>
      language.locale === 'zh-Hans' && key === 'changelog.updateTitle'
        ? simplifiedChinese[key]
        : key,
    locale: language.locale,
  }),
}));
vi.mock('../pages/MotherAgent/mdComponents', () => ({ mdComponents: {} }));
vi.mock('./VoiceInputGuide', () => ({
  VoiceInputGuide: ({ onClose }: { onClose: () => void }) => (
    <button data-guide onClick={onClose}>
      guide
    </button>
  ),
}));
let view: ReactTestRenderer;
let open: (value: boolean) => void;
const closed = vi.fn();
function Harness({ automatic = false }: { automatic?: boolean }) {
  const [isOpen, setOpen] = useState(true);
  useLayoutEffect(() => {
    open = setOpen;
  }, []);
  return (
    <ChangelogDialog
      isOpen={isOpen}
      showVoiceGuideOnClose={automatic}
      onClose={() => {
        closed();
        setOpen(false);
      }}
    />
  );
}
beforeEach(() => {
  vi.useFakeTimers();
  language.locale = 'en';
  closed.mockClear();
  vi.mocked(openExternal).mockReset().mockResolvedValue(undefined);
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')));
  vi.stubGlobal('window', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
});
afterEach(() => {
  act(() => view?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it.each([false, true])(
  'shows the guide only after closing an automatic=%s changelog, including offline notes',
  async (automatic) => {
    await act(async () => {
      view = create(<Harness automatic={automatic} />);
    });
    expect(view.root.findAllByProps({ 'data-guide': true })).toHaveLength(0);
    expect(fetch).toHaveBeenCalledTimes(3);
    const button = view.root.findByType(PageActionButton);
    expect(button.props.children).toBe('changelog.gotIt');
    const dismiss = button.props.onClick;
    act(() => {
      dismiss();
      dismiss();
    });
    act(() => {
      vi.advanceTimersByTime(199);
    });
    expect(closed).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(closed).toHaveBeenCalledOnce();
    expect(view.root.findAllByProps({ 'data-guide': true })).toHaveLength(automatic ? 1 : 0);
    expect(fetch).toHaveBeenCalledTimes(3);
    if (automatic) act(() => view.root.findByProps({ 'data-guide': true }).props.onClick());
    await act(async () => {
      view.update(<Harness automatic={false} />);
      open(true);
    });
    act(() => view.root.findByProps({ 'aria-label': 'btn.close' }).props.onClick());
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(view.root.findAllByProps({ 'data-guide': true })).toHaveLength(0);
  }
);

it.each(['en', 'zh-Hant', 'zh-Hans'])(
  'opens the available footer links from the %s changelog without dismissing it',
  async (locale) => {
    language.locale = locale;
    await act(async () => {
      view = create(<Harness />);
    });
    const footer = view.root.findByType('footer');
    expect(footer.children[footer.children.length - 1]).toBe(footer.findByType(PageActionButton));
    const links = footer.findAllByType('a');
    const expected = [['nav.feedback', 'https://github.com/edison7009/EchoBird/issues']];
    if (locale === 'zh-Hans') {
      expected.push(['国内反馈', 'https://gitcode.com/edison7009/EchoBird/issues/']);
    }
    expect(links.map((link) => [link.props.children, link.props.href])).toEqual(expected);
    expect(openExternal).not.toHaveBeenCalled();
    const preventDefault = vi.fn();
    await act(async () => {
      for (const link of links) {
        link.props.onClick({ preventDefault, currentTarget: { href: link.props.href } });
      }
    });
    expect(preventDefault).toHaveBeenCalledTimes(expected.length);
    expect(vi.mocked(openExternal).mock.calls).toEqual(expected.map(([, url]) => [url]));
    expect(closed).not.toHaveBeenCalled();
  }
);

it('shows both versions and reuses the feedback footer for update confirmation', async () => {
  language.locale = 'zh-Hans';
  const update = {
    fromVersion: '6.4.8',
    toVersion: '6.4.9',
    manifest: { version: '6.4.9', releaseNotesI18n: { 'zh-cn': '## 下载版本日志' } },
    starting: false,
    onStart: vi.fn(),
  };
  await act(async () => {
    view = create(<ChangelogDialog isOpen onClose={closed} update={update} />);
  });
  const dialog = view.root.findByProps({ role: 'dialog' });
  expect(dialog.props['aria-label']).toBe('从 v6.4.8 更新到 v6.4.9，您将体验到的新内容：');
  expect(dialog.findByType('h2').children).toEqual(['下载版本日志']);
  expect(fetch).not.toHaveBeenCalled();
  const footer = view.root.findByType('footer');
  const primary = footer.findByType(PageActionButton);
  expect(footer.findAllByType('a')).toHaveLength(2);
  expect(footer.children[footer.children.length - 1]).toBe(primary);
  expect(primary.props.children).toBe('changelog.startUpdate');
  expect(update.onStart).not.toHaveBeenCalled();
  act(() => primary.props.onClick());
  expect(update.onStart).toHaveBeenCalledOnce();
  expect(closed).not.toHaveBeenCalled();

  act(() => {
    view.update(<ChangelogDialog isOpen onClose={closed} update={{ ...update, starting: true }} />);
  });
  expect(footer.findByType(PageActionButton).findByType('button').props.disabled).toBe(true);
  expect(dialog.findByProps({ 'aria-label': 'btn.close' }).props.disabled).toBe(true);
});

it('keeps keyboard focus in the dialog and restores it to the update button', async () => {
  const doc: { activeElement?: FocusTarget; body?: FocusTarget } = {};
  class FocusTarget {
    isConnected = true;
    focus = vi.fn(() => {
      doc.activeElement = this;
    });
  }

  vi.stubGlobal('HTMLElement', FocusTarget);
  const opener = new FocusTarget();
  const close = new FocusTarget();
  const start = new FocusTarget();
  const panel = Object.assign(new FocusTarget(), {
    ownerDocument: doc,
    contains: (target: FocusTarget) => [panel, close, start].includes(target),
    querySelectorAll: () => [close, start],
  });
  opener.focus();
  await act(async () => {
    view = create(<Harness />, {
      createNodeMock: (element) => (element.props.role === 'dialog' ? panel : null),
    });
  });
  expect(doc.activeElement).toBe(panel);
  const keydown = vi
    .mocked(window.addEventListener)
    .mock.calls.find(([type]) => type === 'keydown')?.[1] as (
    event: Partial<KeyboardEvent>
  ) => void;
  const preventDefault = vi.fn();
  act(() => keydown({ key: 'Tab', preventDefault }));
  expect(doc.activeElement).toBe(close);
  act(() => keydown({ key: 'Tab', shiftKey: true, preventDefault }));
  expect(doc.activeElement).toBe(start);
  act(() => keydown({ key: 'Tab', preventDefault }));
  expect(doc.activeElement).toBe(close);
  expect(preventDefault).toHaveBeenCalledTimes(3);
  act(() => {
    keydown({ key: 'Escape' });
    vi.advanceTimersByTime(200);
  });
  expect(doc.activeElement).toBe(opener);
});

it('allows a cached update to continue when only a newer release manifest is available', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => Response.json({ version: '6.5.0', releaseNotes: '## A later release' }))
  );
  const onStart = vi.fn();
  await act(async () => {
    view = create(
      <ChangelogDialog
        isOpen
        onClose={closed}
        update={{ fromVersion: '6.4.8', toVersion: '6.4.9', starting: false, onStart }}
      />
    );
  });
  expect(view.root.findAllByType('h2')).toHaveLength(0);
  expect(view.root.findAllByType('p').map((node) => node.children.join(''))).toEqual([
    'changelog.error',
  ]);
  const primary = view.root.findByType(PageActionButton).findByType('button');
  expect(primary.props.disabled).toBe(false);
  act(() => primary.props.onClick());
  expect(onStart).toHaveBeenCalledOnce();
});

it('cleans up a pending close when the application unmounts', async () => {
  await act(async () => {
    view = create(<Harness automatic />);
  });
  act(() => view.root.findByProps({ 'aria-label': 'btn.close' }).props.onClick());
  act(() => view.unmount());
  act(() => {
    vi.advanceTimersByTime(500);
  });
  expect(closed).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});

it.each([
  ['zh-Hans', '中文日志'],
  ['en', 'English notes'],
  ['zh-Hant', '繁體日誌'],
  ['ja', '日本語ログ'],
])(
  'shows only the active %s locale when mirrors contain GitHub presentation HTML',
  async (locale, heading) => {
    language.locale = locale;
    const notes = {
      'zh-cn': '## 中文日志',
      en: '## English notes',
      'zh-tw': '## 繁體日誌',
      ja: '## 日本語ログ',
    };
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) =>
        Response.json(
          url.startsWith('https://echobird.ai')
            ? { version: '6.2.1', releaseNotesI18n: notes }
            : {
                version: '6.2.1',
                releaseDate: '2026-11-06T00:08:40Z',
                releaseNotes:
                  '<details><summary>中文</summary>## 镜像旧中文</details><details><summary>English</summary>## Mirror English</details>',
              }
        )
      )
    );
    await act(async () => {
      view = create(<Harness />);
    });
    expect(view.root.findAllByType('h2').map((node) => node.children.join(''))).toEqual([heading]);
    expect(JSON.stringify(view.toJSON())).not.toContain('<details>');
    expect(JSON.stringify(view.toJSON())).not.toContain('T00:08:40Z');
  }
);
