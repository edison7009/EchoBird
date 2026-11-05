import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useNavigationStore as navigation } from './navigationStore';

beforeEach(() => navigation.setState(navigation.getInitialState(), true));
afterEach(() => vi.unstubAllGlobals());

it('keeps developer pages inaccessible until enabled', () => {
  navigation.getState().setActivePage('developer');
  expect(navigation.getState().activePage).toBe('apps');
  expect(navigation.getState().backHistory).toEqual([]);
  navigation.getState().setDeveloperMode(true);
  navigation.getState().setDeveloperTab('plugins');
  navigation.getState().setActivePage('developer');
  expect(navigation.getState().activePage).toBe('developer');
});

it('opens the complete plugin list from an account entry and preserves back navigation', () => {
  const { openAccountPlugins, consumeAccountPluginsRequest, goBack } = navigation.getState();
  openAccountPlugins();
  expect(navigation.getState()).toMatchObject({
    developerMode: true,
    activePage: 'developer',
    developerTab: 'plugins',
    backHistory: ['apps'],
    accountPluginsRequest: expect.any(Number),
  });
  const requestId = navigation.getState().accountPluginsRequest!;
  consumeAccountPluginsRequest(requestId + 1);
  expect(navigation.getState().accountPluginsRequest).toBe(requestId);
  consumeAccountPluginsRequest(requestId);
  expect(navigation.getState().accountPluginsRequest).toBeNull();
  goBack();
  expect(navigation.getState().activePage).toBe('apps');
});

it.each(['back', 'forward', 'active'] as const)(
  'removes developer access and %s history when the mode is disabled',
  (position) => {
    const { setActivePage, setDeveloperMode, setDeveloperTab, goBack, goForward } =
      navigation.getState();
    setDeveloperMode(true);
    setDeveloperTab('plugins');
    setActivePage('developer');
    if (position !== 'active') setActivePage('models');
    if (position === 'forward') {
      goBack();
      goBack();
    }
    setDeveloperMode(false);
    expect(navigation.getState().activePage).not.toBe('developer');
    expect(navigation.getState().backHistory).not.toContain('developer');
    expect(navigation.getState().forwardHistory).not.toContain('developer');
    goBack();
    goForward();
    setActivePage('developer');
    expect(navigation.getState().activePage).not.toBe('developer');
    expect(navigation.getState().developerTab).toBe('plugins');
  }
);

it('visits pages in order and moves back and forward without adding duplicate visits', () => {
  const { setActivePage, goBack, goForward } = navigation.getState();
  setActivePage('models');
  setActivePage('voice');
  expect(navigation.getState().backHistory).toEqual(['apps', 'models']);
  goBack();
  expect(navigation.getState()).toMatchObject({
    activePage: 'models',
    backHistory: ['apps'],
    forwardHistory: ['voice'],
  });
  goBack();
  expect(navigation.getState()).toMatchObject({
    activePage: 'apps',
    backHistory: [],
    forwardHistory: ['voice', 'models'],
  });
  goForward();
  goForward();
  expect(navigation.getState()).toMatchObject({
    activePage: 'voice',
    backHistory: ['apps', 'models'],
    forwardHistory: [],
  });
});

it('keeps invalid history actions and repeated current-page clicks inert', () => {
  const changed = vi.fn();
  const stop = navigation.subscribe(changed);
  const { setActivePage, goBack, goForward } = navigation.getState();
  goBack();
  goForward();
  setActivePage('apps');
  expect(changed).not.toHaveBeenCalled();
  setActivePage('models');
  expect(changed).toHaveBeenCalledOnce();
  goBack();
  changed.mockClear();
  setActivePage('apps');
  goBack();
  expect(changed).not.toHaveBeenCalled();
  expect(navigation.getState().forwardHistory).toEqual(['models']);
  stop();
});

it('discards the forward branch when a different page is opened after going back', () => {
  const { setActivePage, goBack, goForward } = navigation.getState();
  setActivePage('models');
  setActivePage('voice');
  goBack();
  setActivePage('accounts');
  expect(navigation.getState()).toMatchObject({
    activePage: 'accounts',
    backHistory: ['apps', 'models'],
    forwardHistory: [],
  });
  goForward();
  expect(navigation.getState().activePage).toBe('accounts');
});

it('records assistant handoffs once and never replays a prefill through history', () => {
  const { setActivePage, goToMother, clearMotherPrefill, goBack, goForward } =
    navigation.getState();
  setActivePage('voice');
  goToMother('Question: \nSelected text: original', 10);
  expect(navigation.getState()).toMatchObject({
    activePage: 'mother',
    backHistory: ['apps', 'voice'],
    motherPrefillCursor: 10,
  });
  clearMotherPrefill();
  goBack();
  goForward();
  expect(navigation.getState()).toMatchObject({
    activePage: 'mother',
    motherPrefill: undefined,
    motherPrefillCursor: undefined,
  });
  goToMother('Another question', 0);
  expect(navigation.getState().backHistory).toEqual(['apps', 'voice']);
  goBack();
  goForward();
  expect(navigation.getState().motherPrefill).toBeUndefined();
});

it('keeps app signals and passive preference operations outside page history', () => {
  const fetch = vi.fn();
  const setItem = vi.fn();
  vi.stubGlobal('fetch', fetch);
  vi.stubGlobal('localStorage', { setItem });
  const {
    setAgentRunning,
    setUpdateAvailable,
    bumpSshServersVersion,
    setActivePage,
    goBack,
    goForward,
  } = navigation.getState();
  setAgentRunning(true);
  setUpdateAvailable('fixture');
  bumpSshServersVersion();
  expect(navigation.getState().backHistory).toEqual([]);
  setActivePage('models');
  goBack();
  goForward();
  expect(navigation.getState()).toMatchObject({
    agentRunning: true,
    updateAvailable: 'fixture',
    sshServersVersion: 1,
  });
  expect(fetch).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
});

it.each(['aiNews', 'aiCareer', 'localLlm'] as const)(
  'disabling %s redirects its current page, removes history and blocks stale navigation',
  (page) => {
    const nav = navigation.getState();
    nav.setOptionalPages({ aiNews: true, aiCareer: true, localLlm: true });
    nav.setActivePage('models');
    nav.setActivePage(page);
    nav.setActivePage('voice');
    nav.goBack();
    nav.setOptionalPages({ [page]: false });
    expect(navigation.getState().activePage).toBe('apps');
    expect(navigation.getState().backHistory).not.toContain(page);
    expect(navigation.getState().forwardHistory).not.toContain(page);
    nav.setActivePage(page);
    expect(navigation.getState().activePage).toBe('apps');
    nav.goBack();
    nav.goForward();
    expect(navigation.getState().activePage).not.toBe(page);
    nav.setOptionalPages({ [page]: true });
    expect(navigation.getState().activePage).not.toBe(page);
    nav.setActivePage(page);
    expect(navigation.getState().activePage).toBe(page);
  }
);
