// Navigation store — cross-page navigation state & app-wide signals
// Replaces: onGoToMother, onAgentRunningChange, onNewMessage callbacks
// Replaces: page-activated CustomEvent, ssh-servers-changed CustomEvent
// Used by: App.tsx, AppManagerProvider, MotherAgentProvider, SidebarConnected

import { create } from 'zustand';
import type { PageType } from '../components';
import { optionalPagesFromSettings, type OptionalPages } from './optionalPages';

export type AppsTab = 'desktop' | 'install' | 'pets' | 'plugins' | 'usageIsland';
export type SmartAccessTab = 'configuration' | 'statistics';
interface Location {
  page: PageType;
  appsTab: AppsTab;
  smartAccessTab: SmartAccessTab;
  toolTab: 'desktop' | 'install';
}

export type DeveloperTab = 'pets' | 'plugins' | 'statistics' | 'usageIsland';

interface NavigationState {
  activePage: PageType;
  appsTab: AppsTab;
  smartAccessTab: SmartAccessTab;
  toolTab: 'desktop' | 'install';
  backLocations: Location[];
  forwardLocations: Location[];
  setAppsTab: (tab: AppsTab) => void;
  setSmartAccessTab: (tab: SmartAccessTab) => void;
  developerMode: boolean;
  developerModeReady: boolean;
  developerGeneration: number;
  optionalPages: OptionalPages;
  optionalPagesReady: boolean;
  optionalPagesRevision: number;
  developerTab: DeveloperTab;
  accountPluginsRequest: number | null;
  accountPluginsRequestId: number;
  backHistory: PageType[];
  forwardHistory: PageType[];
  motherPrefill: string | undefined;
  motherPrefillCursor: number | undefined;
  agentRunning: boolean;
  updateAvailable: string | null;

  // SSH servers version counter (replaces 'ssh-servers-changed' CustomEvent)
  sshServersVersion: number;

  setActivePage: (page: PageType) => void;
  setDeveloperMode: (enabled: boolean) => void;
  setOptionalPages: (pages: Partial<OptionalPages>) => void;
  setDeveloperTab: (tab: DeveloperTab) => void;
  openAccountPlugins: () => void;
  consumeAccountPluginsRequest: (requestId: number) => void;
  goBack: () => void;
  goForward: () => void;
  goToMother: (prefill: string, cursor?: number) => void;
  clearMotherPrefill: () => void;
  setAgentRunning: (running: boolean) => void;
  setUpdateAvailable: (v: string | null) => void;
  bumpSshServersVersion: () => void;
}

function pageEnabled(state: NavigationState, page: PageType): boolean {
  if (page === 'developer') return state.developerMode;
  if (page === 'aiNews' || page === 'aiCareer' || page === 'localLlm')
    return state.optionalPages[page];
  return true;
}

function location(state: NavigationState): Location {
  return {
    page: state.activePage,
    appsTab: state.appsTab,
    smartAccessTab: state.smartAccessTab,
    toolTab: state.toolTab,
  };
}
function restoreTabs(entry: Location) {
  return { appsTab: entry.appsTab, smartAccessTab: entry.smartAccessTab, toolTab: entry.toolTab };
}
function visitLocation(state: NavigationState, next: Location): Partial<NavigationState> {
  if (!pageEnabled(state, next.page)) return state;
  if (
    state.activePage === next.page &&
    state.appsTab === next.appsTab &&
    state.smartAccessTab === next.smartAccessTab &&
    state.toolTab === next.toolTab
  )
    return state;
  return {
    activePage: next.page,
    appsTab: next.appsTab,
    smartAccessTab: next.smartAccessTab,
    toolTab: next.toolTab,
    backHistory: [...state.backHistory, state.activePage],
    forwardHistory: [],
    backLocations: [...state.backLocations, location(state)],
    forwardLocations: [],
    motherPrefill: undefined,
    motherPrefillCursor: undefined,
  };
}
function visitPage(state: NavigationState, page: PageType): Partial<NavigationState> {
  if (page === 'developer') {
    const tab = state.developerTab;
    return visitLocation(state, {
      ...location(state),
      page: tab === 'statistics' ? 'freeModels' : 'apps',
      ...(tab === 'statistics' ? { smartAccessTab: 'statistics' as const } : { appsTab: tab }),
    });
  }
  return visitLocation(state, { ...location(state), page });
}

export const useNavigationStore = create<NavigationState>((set) => ({
  activePage: 'apps',
  appsTab: 'desktop',
  smartAccessTab: 'configuration',
  toolTab: 'desktop',
  backLocations: [],
  forwardLocations: [],
  setAppsTab: (appsTab) =>
    set((state) =>
      visitLocation(state, {
        ...location(state),
        page: 'apps',
        appsTab,
        toolTab: appsTab === 'desktop' || appsTab === 'install' ? appsTab : state.toolTab,
      })
    ),
  setSmartAccessTab: (smartAccessTab) =>
    set((state) =>
      visitLocation(state, { ...location(state), page: 'freeModels', smartAccessTab })
    ),
  developerMode: false,
  developerModeReady: false,
  developerGeneration: 0,
  optionalPages: optionalPagesFromSettings({}),
  optionalPagesReady: false,
  optionalPagesRevision: 0,
  developerTab: 'pets',
  accountPluginsRequest: null,
  accountPluginsRequestId: 0,
  backHistory: [],
  forwardHistory: [],
  motherPrefill: undefined,
  motherPrefillCursor: undefined,
  agentRunning: false,
  updateAvailable: null,
  sshServersVersion: 0,

  setActivePage: (page) => set((state) => visitPage(state, page)),
  setOptionalPages: (pages) =>
    set((state) => {
      const next = { ...state, optionalPages: { ...state.optionalPages, ...pages } };
      if (
        state.optionalPagesReady &&
        Object.keys(pages).every(
          (page) =>
            state.optionalPages[page as keyof OptionalPages] === pages[page as keyof OptionalPages]
        )
      )
        return state;
      return {
        optionalPages: next.optionalPages,
        optionalPagesReady: true,
        optionalPagesRevision: state.optionalPagesRevision + 1,
        backLocations: state.backLocations.filter((entry) => pageEnabled(next, entry.page)),
        forwardLocations: state.forwardLocations.filter((entry) => pageEnabled(next, entry.page)),
        activePage: pageEnabled(next, state.activePage) ? state.activePage : 'apps',
        backHistory: state.backHistory.filter((page) => pageEnabled(next, page)),
        forwardHistory: state.forwardHistory.filter((page) => pageEnabled(next, page)),
      };
    }),
  setDeveloperMode: (enabled) =>
    set((state) => ({
      developerMode: enabled,
      developerModeReady: true,
      developerGeneration: state.developerGeneration + Number(state.developerMode !== enabled),
      ...(!enabled && {
        activePage: state.activePage === 'developer' ? 'apps' : state.activePage,
        backHistory: state.backHistory.filter((page) => page !== 'developer'),
        forwardHistory: state.forwardHistory.filter((page) => page !== 'developer'),
      }),
    })),
  setDeveloperTab: (developerTab) =>
    set((state) => {
      const destination = visitLocation(state, {
        ...location(state),
        page: developerTab === 'statistics' ? 'freeModels' : 'apps',
        ...(developerTab === 'statistics'
          ? { smartAccessTab: 'statistics' as const }
          : { appsTab: developerTab }),
      });
      return { ...(destination === state ? {} : destination), developerTab };
    }),
  openAccountPlugins: () =>
    set((state) => {
      const next = {
        ...state,
        developerTab: 'plugins' as const,
        appsTab: 'plugins' as const,
        accountPluginsRequestId: state.accountPluginsRequestId + 1,
        accountPluginsRequest: state.accountPluginsRequestId + 1,
      };
      const destination = visitLocation(state, {
        ...location(state),
        page: 'apps',
        appsTab: 'plugins',
      });
      return { ...next, ...(destination === state ? {} : destination) };
    }),
  consumeAccountPluginsRequest: (requestId) =>
    set((state) =>
      state.accountPluginsRequest === requestId ? { accountPluginsRequest: null } : state
    ),
  goBack: () =>
    set((state) => {
      const page = state.backHistory[state.backHistory.length - 1];
      if (!page) return state;
      return {
        ...restoreTabs(state.backLocations[state.backLocations.length - 1] ?? location(state)),
        activePage: page,
        backLocations: state.backLocations.slice(0, -1),
        forwardLocations: [...state.forwardLocations, location(state)],
        backHistory: state.backHistory.slice(0, -1),
        forwardHistory: [...state.forwardHistory, state.activePage],
        motherPrefill: undefined,
        motherPrefillCursor: undefined,
      };
    }),
  goForward: () =>
    set((state) => {
      const page = state.forwardHistory[state.forwardHistory.length - 1];
      if (!page) return state;
      return {
        ...restoreTabs(
          state.forwardLocations[state.forwardLocations.length - 1] ?? location(state)
        ),
        activePage: page,
        forwardLocations: state.forwardLocations.slice(0, -1),
        backLocations: [...state.backLocations, location(state)],
        backHistory: [...state.backHistory, state.activePage],
        forwardHistory: state.forwardHistory.slice(0, -1),
        motherPrefill: undefined,
        motherPrefillCursor: undefined,
      };
    }),
  goToMother: (prefill, cursor) =>
    set((state) => ({
      ...visitPage(state, 'mother'),
      motherPrefill: prefill,
      motherPrefillCursor: cursor,
    })),
  // One-shot consume: MotherAgentProvider clears this right after it fills
  // the chat input, so revisiting the Mother page doesn't re-fill a stale
  // "安装XX" prompt the user already sent or cleared.
  clearMotherPrefill: () => set({ motherPrefill: undefined, motherPrefillCursor: undefined }),
  setAgentRunning: (running) => set({ agentRunning: running }),
  setUpdateAvailable: (v) => set({ updateAvailable: v }),
  bumpSshServersVersion: () => set((s) => ({ sshServersVersion: s.sshServersVersion + 1 })),
}));
