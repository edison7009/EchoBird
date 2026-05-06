// useChatPersistence — localStorage-backed chat history for Mother Agent.
// History is small enough to live entirely in localStorage; pagination
// still exists in the API but always serves from memory after first load.

import { useState, useRef, useEffect, useCallback } from 'react';

/** Disk message format (we persist only role + content) */
export interface DiskMsg {
    role: string;
    content: string;
}

export interface UseChatPersistenceOptions<T> {
    /** localStorage key prefix (null = disabled) */
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
    diskTotal: number;
    showSkeleton: boolean;
    displayCount: number;
    resetDisplayCount: () => void;
    loadInitial: () => Promise<void>;
    handleScrollPagination: (container: HTMLDivElement) => void;
    loadOlderChat: () => Promise<number>;
    clearHistory: () => Promise<void>;
}

const STORAGE_PREFIX = 'echobird:chat:';

function readStored(key: string): DiskMsg[] {
    try {
        const raw = localStorage.getItem(STORAGE_PREFIX + key);
        if (!raw) return [];
        const parsed = JSON.parse(raw);
        return Array.isArray(parsed) ? parsed : [];
    } catch {
        return [];
    }
}

function writeStored(key: string, msgs: DiskMsg[]) {
    try {
        localStorage.setItem(STORAGE_PREFIX + key, JSON.stringify(msgs));
    } catch { /* quota / private mode — ignore */ }
}

export function useChatPersistence<T>(options: UseChatPersistenceOptions<T>): UseChatPersistenceResult {
    const {
        diskKey,
        messages,
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

    const messagesRef = useRef(messages);
    messagesRef.current = messages;
    const diskKeyRef = useRef(diskKey);
    diskKeyRef.current = diskKey;
    const fromDiskRef = useRef(fromDisk);
    fromDiskRef.current = fromDisk;

    // Keep displayCount >= messages.length so new messages are always visible
    useEffect(() => {
        if (messages.length > displayCount) {
            setDisplayCount(messages.length);
        }
    }, [messages.length, displayCount]);

    // Debounced save to localStorage
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
            writeStored(key, diskMsgs);
            setDiskTotal(diskMsgs.length);
        }, debounceMs);

        return () => {
            if (saveDebounceRef.current) clearTimeout(saveDebounceRef.current);
        };
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [messages, diskKey]);

    // Load full history from localStorage on mount
    const loadInitial = useCallback(async () => {
        if (!diskKeyRef.current) return;
        const stored = readStored(diskKeyRef.current);
        if (stored.length > 0) {
            setMessages(stored.map(m => fromDiskRef.current(m)));
            setDiskTotal(stored.length);
            setDisplayCount(pageSize);
        }
    }, [pageSize, setMessages]);

    // localStorage holds the full list, so older messages are already in memory
    const loadOlderChat = useCallback(async (): Promise<number> => 0, []);

    const handleScrollPagination = useCallback((container: HTMLDivElement) => {
        if (container.scrollTop !== 0) return;
        const total = messagesRef.current.length;
        if (displayCount >= total) return;

        setShowSkeleton(true);
        const prevH = container.scrollHeight;
        setTimeout(() => {
            setShowSkeleton(false);
            setDisplayCount(c => Math.min(c + pageSize, messagesRef.current.length));
            requestAnimationFrame(() => {
                container.scrollTop = container.scrollHeight - prevH;
            });
        }, 300);
    }, [displayCount, pageSize]);

    const resetDisplayCount = useCallback(() => {
        setDisplayCount(pageSize);
    }, [pageSize]);

    const clearHistory = useCallback(async () => {
        if (!diskKeyRef.current) return;
        setDiskTotal(0);
        try {
            localStorage.removeItem(STORAGE_PREFIX + diskKeyRef.current);
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
