import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, expect, it, vi } from 'vitest';
import { I18nProvider, useI18n } from './useI18n';

const mocks = vi.hoisted(() => ({
  pendingGerman: Promise.resolve(),
  loads: new Map<string, Promise<void>>(),
  save: vi.fn(async (_settings: { locale: string }) => {}),
}));

vi.mock('../api/tauri', () => ({ saveSettings: mocks.save }));
vi.mock('../i18n', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../i18n')>();
  return {
    ...actual,
    loadLocale: (locale: string) => {
      const pending = (locale === 'de' ? mocks.pendingGerman : Promise.resolve()).then(() =>
        actual.loadLocale(locale)
      );
      mocks.loads.set(locale, pending);
      return pending;
    },
  };
});

let view: ReactTestRenderer | undefined;
function Probe() {
  const { locale, t, setLocale } = useI18n();
  return (
    <>
      <output lang={locale}>{t('settings.interfaceLanguage')}</output>
      <button type="button" lang="de" onClick={() => setLocale('de-DE')}>
        Deutsch
      </button>
      <button type="button" lang="vi" onClick={() => setLocale('vi-VN')}>
        Tiếng Việt
      </button>
    </>
  );
}

const displayedLanguage = () => view!.root.findByType('output');
const chooseLanguage = (lang: string) =>
  view!.root.findByProps({ lang, type: 'button' }).props.onClick();

afterEach(() => {
  act(() => view?.unmount());
  view = undefined;
  vi.unstubAllGlobals();
});

it('keeps the current text while loading and saves only the latest selected language', async () => {
  const documentElement = { lang: 'en' };
  const store = vi.fn();
  vi.stubGlobal('document', { documentElement });
  vi.stubGlobal('window', new EventTarget());
  vi.stubGlobal('localStorage', { setItem: store });
  let finishGerman!: () => void;
  mocks.pendingGerman = new Promise<void>((resolve) => (finishGerman = resolve));
  await act(async () => {
    view = create(
      <I18nProvider initialLocale="en">
        <Probe />
      </I18nProvider>
    );
  });

  await act(async () => chooseLanguage('de'));
  expect(displayedLanguage().props.children).toBe('Interface language');
  await act(async () => {
    chooseLanguage('vi');
    await mocks.loads.get('vi');
  });
  expect(displayedLanguage().props.lang).toBe('vi');
  expect(displayedLanguage().props.children).toBe('Ngôn ngữ giao diện');
  expect(documentElement.lang).toBe('vi');
  await act(async () => {
    finishGerman();
    await mocks.loads.get('de');
  });
  expect(displayedLanguage().props.lang).toBe('vi');
  expect(store).toHaveBeenCalledExactlyOnceWith('echobird-locale', 'vi');
  expect(mocks.save).toHaveBeenCalledExactlyOnceWith({ locale: 'vi' });

  const [saved] = mocks.save.mock.calls[0]!;
  await act(async () => {
    view?.unmount();
    view = create(
      <I18nProvider initialLocale={saved.locale}>
        <Probe />
      </I18nProvider>
    );
  });
  expect(displayedLanguage().props.children).toBe('Ngôn ngữ giao diện');
});
