import ReactDOM from 'react-dom/client';
import { I18nContext } from '../hooks/i18nContext';
import { loadLocale, translate } from '../i18n';
import { getColorTheme } from '../data/colorThemes';
import { AssistantDemo, ModelsDemo, PetsDemo } from './FeatureDemos';
import '../index.css';
import './feature-demos.css';
import { demoLocale } from './demoLocale';
const locale = demoLocale();
document.documentElement.lang = locale === 'en' ? 'en' : 'zh-CN';

const palette = getColorTheme('oatgray').dark;
for (const [key, value] of Object.entries({
  '--bg-base-rgb': palette.canvas,
  '--bg-terminal-rgb': palette.shell,
  '--bg-surface-rgb': palette.secondary,
  '--bg-elevated-rgb': palette.tertiary,
  '--bg-input-rgb': palette.input,
}))
  document.documentElement.style.setProperty(
    key,
    value
      .slice(1)
      .match(/../g)!
      .map((channel) => parseInt(channel, 16))
      .join(' ')
  );

const mode = new URLSearchParams(location.search).get('mode');
void loadLocale(locale).then(() => {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <I18nContext.Provider
      value={{ locale, setLocale: () => {}, t: (key) => translate(key, locale) }}
    >
      {mode === 'models' ? <ModelsDemo /> : mode === 'assistant' ? <AssistantDemo /> : <PetsDemo />}
    </I18nContext.Provider>
  );
});
