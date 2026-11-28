import ReactDOM from 'react-dom/client';
import { I18nContext } from '../hooks/i18nContext';
import { loadLocale } from '../i18n';
import { getColorTheme } from '../data/colorThemes';
import { DemoDesktop } from './DemoDesktop';
import { DemoUsageIsland } from './DemoUsageIsland';
import '../index.css';
import { demoLocale, demoTranslate } from './demoLocale';
const locale = demoLocale();
document.documentElement.lang = locale === 'zh-Hans' ? 'zh-CN' : locale;
if (locale === 'tr')
  document.title =
    document.body.dataset.demo === 'usage-island'
      ? 'EchoBird · Kullanım Adası'
      : 'EchoBird · Etkileşimli masaüstü demosu';

// Use the desktop's palette without initializing native settings or persistence.
const palette = getColorTheme('oatgray').dark;
const channels = (hex: string) =>
  hex
    .slice(1)
    .match(/../g)!
    .map((value) => parseInt(value, 16))
    .join(' ');
for (const [key, value] of Object.entries({
  '--bg-base-rgb': palette.canvas,
  '--bg-terminal-rgb': palette.shell,
  '--bg-surface-rgb': palette.secondary,
  '--bg-elevated-rgb': palette.tertiary,
  '--bg-input-rgb': palette.input,
}))
  document.documentElement.style.setProperty(key, channels(value));

void Promise.all([
  loadLocale(locale),
  document.body.dataset.demo === 'usage-island' ? Promise.resolve() : import('./demo.css'),
]).then(() => {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <I18nContext.Provider
      value={{ locale, setLocale: () => {}, t: (key) => demoTranslate(key, locale) }}
    >
      {document.body.dataset.demo === 'usage-island' ? <DemoUsageIsland /> : <DemoDesktop />}
    </I18nContext.Provider>
  );
});
