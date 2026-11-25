// All interactive website demos run in a browser using only demo data.
interface DemoSettings {
  locale?: string;
  themeMode?: 'light' | 'dark';
  colorTheme?: string;
}

const SETTINGS_KEY = 'echobird-website-demo-settings';

export async function getSettings(): Promise<DemoSettings> {
  try {
    return JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') as DemoSettings;
  } catch {
    return {};
  }
}

export async function saveSettings(settings: DemoSettings): Promise<void> {
  const previous = await getSettings();
  localStorage.setItem(SETTINGS_KEY, JSON.stringify({ ...previous, ...settings }));
}

export async function getSystemApplicationIcon(_id: string): Promise<null> {
  return null;
}

export async function openExternal(href: string): Promise<void> {
  const url = new URL(href, window.location.href);
  if (url.protocol === 'https:' || url.protocol === 'http:') {
    window.open(url.href, '_blank', 'noopener,noreferrer');
  }
}
