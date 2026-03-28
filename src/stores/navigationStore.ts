// Navigation store — cross-page navigation state & app-wide signals
// Replaces: onGoToMother, onAgentRunningChange, onNewMessage callbacks
// Replaces: page-activated CustomEvent, chat-error CustomEvent, ssh-servers-changed CustomEvent
// Used by: App.tsx, AppManagerProvider, MotherAgentProvider, SidebarConnected, Channels

import { create } from 'zustand';
import type { PageType } from '../components';

interface NavigationState {
    activePage: PageType;
    motherPrefill: string | undefined;
    agentRunning: boolean;
    motherNewMessage: boolean;
    updateAvailable: string | null;

    // Chat error flash counter (replaces 'chat-error' CustomEvent)
    flashCount: number;
    // SSH servers version counter (replaces 'ssh-servers-changed' CustomEvent)
    sshServersVersion: number;

    setActivePage: (page: PageType) => void;
    goToMother: (prefill: string) => void;
    setAgentRunning: (running: boolean) => void;
    setMotherNewMessage: (v: boolean) => void;
    clearMotherBadge: () => void;
    setUpdateAvailable: (v: string | null) => void;
    incrementFlashCount: (count?: number) => void;
    bumpSshServersVersion: () => void;
}

export const useNavigationStore = create<NavigationState>((set) => ({
    activePage: 'models',
    motherPrefill: undefined,
    agentRunning: false,
    motherNewMessage: false,
    updateAvailable: null,
    flashCount: 0,
    sshServersVersion: 0,

    setActivePage: (page) => set({ activePage: page }),
    goToMother: (prefill) => set({ activePage: 'mother', motherPrefill: prefill }),
    setAgentRunning: (running) => set({ agentRunning: running }),
    setMotherNewMessage: (v) => set({ motherNewMessage: v }),
    clearMotherBadge: () => set({ motherNewMessage: false }),
    setUpdateAvailable: (v) => set({ updateAvailable: v }),
    incrementFlashCount: (count = 1) => set(s => ({ flashCount: s.flashCount + count })),
    bumpSshServersVersion: () => set(s => ({ sshServersVersion: s.sshServersVersion + 1 })),
}));
