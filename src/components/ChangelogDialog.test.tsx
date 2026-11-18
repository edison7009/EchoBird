import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { useLayoutEffect, useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ChangelogDialog } from './ChangelogDialog';

const language = vi.hoisted(() => ({ locale: 'en' }));
vi.mock('../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: language.locale }),
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
    const dismiss = view.root.findByType('button').props.onClick;
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
    act(() => view.root.findByType('button').props.onClick());
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(view.root.findAllByProps({ 'data-guide': true })).toHaveLength(0);
  }
);

it('cleans up a pending close when the application unmounts', async () => {
  await act(async () => {
    view = create(<Harness automatic />);
  });
  act(() => view.root.findByType('button').props.onClick());
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
