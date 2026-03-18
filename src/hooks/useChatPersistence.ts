// useChatPersistence — Shared hook for chat history disk persistence.
// Used by both Channels and Mother Agent pages.
// Handles: debounced save, paginated load, two-phase scroll, skeleton state.

import { useState, useRef, useEffect, useCallback } from 'react';
import { channelHistoryLoad, channelHistorySave, channelHistoryClear } from '../api/tauri';

/** Disk message format (backend stores only role + content) */
export interface DiskMsg {
    role: string;
    content: string;
}

export interface UseChatPersistenceOptions<T> {
    /** channel_history key (null = disabled) */
    diskKey: string | null;
    /** Current in-memory messages */
    messages: T[];
    /** Prepend older messages from disk */
    prependMessages: (older: T[]) => void;
    /** Replace all messages (used on initial load) */
    setMessages: (msgs: T[]) => void;
    /** Convert app message → disk format (return null to skip) */
    toDisk: (msg: T) => DiskMsg | null;
    /** Convert disk format → app message */
    fromDisk: (msg: DiskMsg) => T;
    /** Messages per page (default 30) */
    pageSize?: number;
    /** Debounce delay in ms (default 800) */
    debounceMs?: number;
}

export interface UseChatPersistenceResult {
    /** Total messages stored on disk */
    diskTotal: number;
    /** Whether skeleton loading indicator should show */
    showSkeleton: boolean;
    /** How many messages to display (for UI slicing) */
    displayCount: number;
    /** Reset display count (call when switching channels/servers) */
    resetDisplayCount: () => void;
    /** Load initial page from disk */
    loadInitial: () => Promise<void>;
    /** Handle scroll event — call from onScroll on chat container */
    handleScrollPagination: (container: HTMLDivElement) => void;
    /** Load older messages from disk (Phase 2) */
    loadOlderChat: () => Promise<number>;
    /** Clear all persisted history */
    clearHistory: () => Promise<void>;
}

export function useChatPersistence<T>(options: UseChatPersistenceOptions<T>): UseChatPersistenceResult {
    const {
        diskKey,
        messages,
        prependMessages,
        setMessages,
        toDisk,
        fromDisk,
        pageSize = 30,
        debounceMs = 800,
    } = options;

    const [diskTotal, setDiskTotal] = useState(0);
    const [showSkeleton, setShowSkeleton] = useState(false);
    const [displayCount, setDisplayCount] = useState(pageSize);
    const saveDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Stable refs to avoid stale closure issues
    const messagesRef = useRef(messages);
    messagesRef.current = messages;
    const diskKeyRef = useRef(diskKey);
    diskKeyRef.current = diskKey;
    const toDiskRef = useRef(toDisk);
    toDiskRef.current = toDisk;
    const fromDiskRef = useRef(fromDisk);
    fromDiskRef.current = fromDisk;

    // ── Auto-grow displayCount to cover all in-memory messages ──────────────
    // During active chat, new messages are added beyond the initial pageSize.
    // This ensures they are always visible without requiring manual scroll-up.
    useEffect(() => {
        if (messages.length > displayCount) {
            setDisplayCount(messages.length);
        }
    }, [messages.length, displayCount]);

    // ── Debounced save ──────────────────────────────────────────────────────
    useEffect(() => {
        if (!diskKey || messages.length === 0) return;
        const diskMsgs: DiskMsg[] = [];
        for (const m of messages) {
            const d = toDisk(m);
            if (d) diskMsgs.push(d);
        }
        if (diskMsgs.length === 0) return;

        if (saveDebounceRef.current) clearTimeout(saveDebounceRef.current);
        const key = diskKey;
        saveDebounceRef.current = setTimeout(() => {
            channelHistorySave(key, diskMsgs).catch(() => { });
        }, debounceMs);

        return () => {
            if (saveDebounceRef.current) clearTimeout(saveDebounceRef.current);
        };
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [messages, diskKey]);

    // ── Load initial page ───────────────────────────────────────────────────
    const loadInitial = useCallback(async () => {
        if (!diskKeyRef.current) return;
        try {
            const result = await channelHistoryLoad(diskKeyRef.current, 0, pageSize);
            if (result.total > 0) setDiskTotal(result.total);
            if (result.messages.length > 0) {
                setMessages(result.messages.map(m => fromDiskRef.current(m)));
                setDisplayCount(pageSize);
            }
        } catch { /* ignore */ }
    }, [pageSize, setMessages]);

    // ── Load older batch from disk (Phase 2) ────────────────────────────────
    const loadOlderChat = useCallback(async (): Promise<number> => {
        if (!diskKeyRef.current) return 0;
        const alreadyLoaded = messagesRef.current.length;
        try {
            const result = await channelHistoryLoad(diskKeyRef.current, alreadyLoaded, pageSize);
            if (result.messages.length === 0) return 0;
            const older = result.messages.map(m => fromDiskRef.current(m));
            prependMessages(older);
            return older.length;
        } catch {
            return 0;
        }
    }, [pageSize, prependMessages]);

    // ── Two-phase scroll handler ────────────────────────────────────────────
    const handleScrollPagination = useCallback((container: HTMLDivElement) => {
        if (container.scrollTop !== 0) return;

        const currentMessages = messagesRef.current;

        // Phase 1: show more in-memory messages
        if (displayCount < currentMessages.length) {
            setShowSkeleton(true);
            const prevH = container.scrollHeight;
            setTimeout(() => {
                setShowSkeleton(false);
                setDisplayCount(c => Math.min(c + pageSize, messagesRef.current.length));
                requestAnimationFrame(() => {
                    container.scrollTop = container.scrollHeight - prevH;
                });
            }, 300);
            return;
        }

        // Phase 2: fetch from disk
        const alreadyLoaded = currentMessages.length;
        if (alreadyLoaded >= diskTotal) return;

        setShowSkeleton(true);
        const prevH = container.scrollHeight;
        loadOlderChat().then(count => {
            setShowSkeleton(false);
            if (count > 0) {
                setDisplayCount(c => c + count);
                requestAnimationFrame(() => {
                    container.scrollTop = container.scrollHeight - prevH;
                });
            }
        }).catch(() => { setShowSkeleton(false); });
    }, [displayCount, diskTotal, pageSize, loadOlderChat]);

    // ── Reset display count ─────────────────────────────────────────────────
    const resetDisplayCount = useCallback(() => {
        setDisplayCount(pageSize);
    }, [pageSize]);

    // ── Clear ───────────────────────────────────────────────────────────────
    const clearHistory = useCallback(async () => {
        if (!diskKeyRef.current) return;
        setDiskTotal(0);
        try {
            await channelHistoryClear(diskKeyRef.current);
        } catch { /* ignore */ }
    }, []);

    return {
        diskTotal,
        showSkeleton,
        displayCount,
        resetDisplayCount,
        loadInitial,
        handleScrollPagination,
        loadOlderChat,
        clearHistory,
    };
}
