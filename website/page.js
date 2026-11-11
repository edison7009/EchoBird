import { configureDownload, shouldShowChineseMirrors } from './download.js';
import { initializeSiteLocale } from './site-locale.js';
import { uiStrings } from './site-translations.js';
const siteLocale = initializeSiteLocale();
const siteUi = uiStrings[siteLocale];

const stage = document.querySelector('.live-demo-stage');
let lastWidth = 0;
new ResizeObserver(([entry]) => {
  const width = entry.contentRect.width;
  if (width === lastWidth) return;
  lastWidth = width;
  requestAnimationFrame(() => stage.style.setProperty('--demo-scale', width / 1309));
}).observe(stage);

configureDownload(document.getElementById('heroDownload'), navigator);
let savedDownloadLocale;
try {
  savedDownloadLocale = localStorage.getItem('echobird.lang');
} catch {
  /* Browser language still works when storage is unavailable. */
}
const cloudDownloads = document.querySelector('.cloud-downloads');
if (cloudDownloads)
  cloudDownloads.hidden = !shouldShowChineseMirrors(navigator, savedDownloadLocale, siteLocale);

// Run the showcase only while it is actually visible in the marketing page.
const usageFrame = document.querySelector('[data-usage-preview]');
if (usageFrame) {
  let usageVisible = false;
  const syncUsage = () =>
    usageFrame.contentWindow?.postMessage(
      { type: 'echobird-usage-preview', visible: usageVisible && !document.hidden },
      window.location.origin
    );
  const observer = new IntersectionObserver(
    ([entry]) => {
      usageVisible = entry.isIntersecting && entry.intersectionRatio >= 0.15;
      syncUsage();
    },
    { threshold: 0.15 }
  );
  observer.observe(usageFrame);
  const onUsageReady = (event) => {
    if (
      event.source === usageFrame.contentWindow &&
      event.origin === window.location.origin &&
      event.data?.type === 'echobird-usage-ready'
    )
      syncUsage();
  };
  usageFrame.addEventListener('load', syncUsage);
  window.addEventListener('message', onUsageReady);
  document.addEventListener('visibilitychange', syncUsage);
  import.meta.hot?.dispose(() => {
    observer.disconnect();
    usageFrame.removeEventListener('load', syncUsage);
    window.removeEventListener('message', onUsageReady);
    document.removeEventListener('visibilitychange', syncUsage);
  });
}

const marquee = document.querySelector('.tool-marquee');
if (marquee) {
  let visible = false;
  const sync = () => marquee.classList.toggle('is-running', visible && !document.hidden);
  const observer = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    sync();
  });
  observer.observe(marquee);
  document.addEventListener('visibilitychange', sync);
  import.meta.hot?.dispose(() => {
    observer.disconnect();
    document.removeEventListener('visibilitychange', sync);
  });
}

// Copy only after an explicit click; installing remains in the user's terminal.
for (const button of document.querySelectorAll('[data-copy-command]')) {
  button.addEventListener('click', async () => {
    const command = document.getElementById(button.dataset.copyCommand);
    const feedback = document.querySelector('.copy-feedback');
    if (!command) return;
    try {
      await navigator.clipboard.writeText(command.textContent.trim());
      feedback.textContent = siteUi.copySuccess;
    } catch {
      feedback.textContent = siteUi.copyFailure;
    }
  });
}

// Feature demos share the same visibility-driven playback as the Usage Island.
for (const frame of document.querySelectorAll('[data-feature-preview]')) {
  let visible = false;
  const sync = () =>
    frame.contentWindow?.postMessage(
      { type: 'echobird-feature-preview', visible: visible && !document.hidden },
      location.origin
    );
  const observer = new IntersectionObserver(
    ([entry]) => {
      visible = entry.isIntersecting && entry.intersectionRatio >= 0.15;
      sync();
    },
    { threshold: 0.15 }
  );
  const ready = (event) => {
    if (
      event.source === frame.contentWindow &&
      event.origin === location.origin &&
      event.data?.type === 'echobird-feature-ready'
    )
      sync();
  };
  observer.observe(frame);
  frame.addEventListener('load', sync);
  window.addEventListener('message', ready);
  document.addEventListener('visibilitychange', sync);
  import.meta.hot?.dispose(() => {
    observer.disconnect();
    frame.removeEventListener('load', sync);
    window.removeEventListener('message', ready);
    document.removeEventListener('visibilitychange', sync);
  });
}

// A short desktop wheel gesture advances one feature. Leave the rest of the page free.
const focusLayout = matchMedia('(min-width: 801px) and (min-height: 720px)');
const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
const focusFeatures = [...document.querySelectorAll('.feature')];
let wheelTotal = 0;
let lastWheel = 0;
let focusLockedUntil = 0;
let gestureUsed = false;
const onFeatureWheel = (event) => {
  if (
    !focusLayout.matches ||
    reducedMotion.matches ||
    event.ctrlKey ||
    event.shiftKey ||
    Math.abs(event.deltaX) >= Math.abs(event.deltaY)
  )
    return;
  const centers = focusFeatures.map((element) => {
    const rect = element.getBoundingClientRect();
    return rect.top + rect.height / 2 - innerHeight / 2;
  });
  const nearest = centers.reduce(
    (best, distance, index) => (Math.abs(distance) < Math.abs(centers[best]) ? index : best),
    0
  );
  if (Math.abs(centers[nearest]) > innerHeight * 0.55) return;
  // Text fields and independently scrollable areas retain their normal wheel behavior.
  let target = event.target?.nodeType === 1 ? event.target : null;
  if (target?.closest('input, textarea, select, [role="dialog"]')) return;
  while (target && target !== target.ownerDocument.body) {
    const style = target.ownerDocument.defaultView.getComputedStyle(target);
    if (
      /(auto|scroll)/.test(style.overflowY) &&
      target.scrollHeight > target.clientHeight + 1 &&
      ((event.deltaY > 0 && target.scrollTop + target.clientHeight < target.scrollHeight - 1) ||
        (event.deltaY < 0 && target.scrollTop > 0))
    )
      return;
    target = target.parentElement;
  }
  event.preventDefault();
  const now = performance.now();
  const newGesture = now - lastWheel > 180;
  lastWheel = now;
  if (newGesture) {
    wheelTotal = 0;
    gestureUsed = false;
  }
  if (now < focusLockedUntil || gestureUsed) return;
  const delta =
    event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? innerHeight : 1);
  if (Math.sign(delta) !== Math.sign(wheelTotal)) wheelTotal = 0;
  wheelTotal += delta;
  if (Math.abs(wheelTotal) < 36) return;
  const direction = Math.sign(wheelTotal);
  const next = Math.abs(centers[nearest]) > 40 ? nearest : nearest + direction;
  const destination =
    next < 0
      ? document.querySelector('.relay-recommendations')
      : next >= focusFeatures.length
        ? document.querySelector('#download')
        : focusFeatures[next];
  if (!destination) return;
  gestureUsed = true;
  focusLockedUntil = now + 850;
  wheelTotal = 0;
  destination.scrollIntoView({
    behavior: 'smooth',
    block: 'center',
  });
};
window.addEventListener('wheel', onFeatureWheel, { passive: false });
// Wheel events inside same-origin preview frames do not bubble to the parent document.
const featureWheelFrames = [...document.querySelectorAll('.feature iframe')];
const boundFrameWindows = new Set();
const bindFeatureWheel = (event) => {
  const frame = event.currentTarget;
  const frameWindow = frame.contentWindow;
  if (!frameWindow) return;
  frameWindow.removeEventListener('wheel', onFeatureWheel);
  frameWindow.addEventListener('wheel', onFeatureWheel, { passive: false });
  boundFrameWindows.add(frameWindow);
};
for (const frame of featureWheelFrames) {
  frame.addEventListener('load', bindFeatureWheel);
  if (frame.contentDocument?.readyState === 'complete') bindFeatureWheel({ currentTarget: frame });
}
import.meta.hot?.dispose(() => {
  window.removeEventListener('wheel', onFeatureWheel);
  for (const frame of featureWheelFrames) frame.removeEventListener('load', bindFeatureWheel);
  for (const frameWindow of boundFrameWindows)
    frameWindow.removeEventListener('wheel', onFeatureWheel);
});

// Text navigation follows the section nearest the center without moving keyboard focus.
const sectionNavigation = document.querySelector('.section-navigation');
const sectionLinks = [...document.querySelectorAll('.section-navigation a')];
const sectionJumpLinks = [
  ...sectionLinks,
  document.querySelector('.navlinks a[href="#usage-island"]'),
].filter(Boolean);
const sectionTargets = sectionLinks.map((link) =>
  link.hash === '#home' ? document.querySelector('.hero') : document.querySelector(link.hash)
);
let navigationFrame = 0;
const updateSectionNavigation = () => {
  navigationFrame = 0;
  let nearest = 0;
  let distance = Infinity;
  sectionTargets.forEach((section, index) => {
    if (!section) return;
    const rect = section.getBoundingClientRect();
    const nextDistance = Math.abs(rect.top + rect.height / 2 - innerHeight / 2);
    if (nextDistance < distance) {
      distance = nextDistance;
      nearest = index;
    }
  });
  // Visibility follows the hero boundary, independently of which link is active.
  const hero = document.querySelector('.hero');
  if (sectionNavigation)
    sectionNavigation.hidden = !hero || hero.getBoundingClientRect().bottom > 0;
  sectionLinks.forEach((link, index) => {
    if (index === nearest) link.setAttribute('aria-current', 'location');
    else link.removeAttribute('aria-current');
  });
};
const scheduleSectionNavigation = () => {
  if (!navigationFrame) navigationFrame = requestAnimationFrame(updateSectionNavigation);
};
const navigateToSection = (event) => {
  const link = event.currentTarget;
  const destination = document.querySelector(link.hash);
  if (!destination) return;
  event.preventDefault();
  focusLockedUntil = performance.now() + 850;
  destination.scrollIntoView({
    behavior: reducedMotion.matches ? 'instant' : 'smooth',
    block: link.hash === '#home' ? 'start' : 'center',
  });
  history.replaceState(null, '', link.hash);
};
for (const link of sectionJumpLinks) link.addEventListener('click', navigateToSection);
window.addEventListener('scroll', scheduleSectionNavigation, { passive: true });
window.addEventListener('resize', scheduleSectionNavigation);
updateSectionNavigation();
import.meta.hot?.dispose(() => {
  for (const link of sectionJumpLinks) link.removeEventListener('click', navigateToSection);
  window.removeEventListener('scroll', scheduleSectionNavigation);
  window.removeEventListener('resize', scheduleSectionNavigation);
  cancelAnimationFrame(navigationFrame);
});
