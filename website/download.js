const platformIcons = {
  windows: new URL('./assets/platform-windows.svg', import.meta.url).href,
  'macos-arm': new URL('./assets/platform-macos.svg', import.meta.url).href,
  'linux-deb': new URL('./assets/platform-linux.svg', import.meta.url).href,
};

// Match docs/index.html's published OS choices; keep installer resolution on its worker.
export function downloadForPlatform(platform) {
  if (/Windows/i.test(platform)) return { route: 'windows', name: 'Windows', extension: '.exe' };
  if (/Mac OS X|Macintosh|macOS/i.test(platform))
    return { route: 'macos-arm', name: 'macOS', extension: '.dmg' };
  if (/Linux/i.test(platform)) return { route: 'linux-deb', name: 'Linux', extension: '.deb' };
  return null;
}

export function configureDownload(button, browser) {
  if (!button) return;
  const download = downloadForPlatform(browser.userAgentData?.platform || browser.userAgent || '');
  if (!download) return;
  // An absolute URL also works in the local prototype, which has no download worker.
  button.href = `https://echobird.ai/download/${download.route}`;
  button.querySelector('[data-download-name]').textContent = download.name;
  const icon = button.querySelector('[data-platform-icon]');
  if (icon) {
    const image = button.ownerDocument.createElement('img');
    image.alt = '';
    image.src = platformIcons[download.route];
    icon.replaceChildren(image);
  }
  const extension = button.querySelector('.download-ext');
  extension.textContent = download.extension;
  extension.hidden = false;
}

// Match the published site's saved-language preference and browser-language fallback.
export function shouldShowChineseMirrors(browser, savedLocale, pathLanguage) {
  if (pathLanguage === 'zh' || pathLanguage === 'en') return pathLanguage === 'zh';
  if (savedLocale === 'zh-CN' || savedLocale === 'en') return savedLocale === 'zh-CN';
  return (browser.language || 'en').toLowerCase().startsWith('zh');
}
