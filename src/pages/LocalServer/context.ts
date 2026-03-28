// context.ts — LocalServer shared context + types
import React, { createContext, useContext } from 'react';

// ─── Types ───

export type EngineStatus = 'checking' | 'ready' | 'not-installed' | 'downloading' | 'error' | 'update-available';

export interface GgufFileEntry {
    fileName: string;
    filePath: string;
    fileSize: number;
}

// ─── Context ───

export interface LocalServerContextValue {
    // Model selection
    selectedModelPath: string | null;
    setSelectedModelPath: (path: string | null) => void;
    // GGUF / HF model files
    ggufFiles: GgufFileEntry[];
    isScanning: boolean;
    rescanModels: (runtime?: string) => void;
    // Model dirs
    modelsDirs: string[];
    // Current runtime (shared between Main and Panel)
    runtime: string;
    setRuntime: (v: string) => void;
    // Server runtime state (for bottom bar)
    serverRunning: boolean;
    setServerRunning: (v: boolean) => void;
    serverPort: number;
    setServerPort: (v: number) => void;
    serverModelName: string;
    setServerModelName: (v: string) => void;
    serverApiKey: string;
    setServerApiKey: (v: string) => void;
}

export const LocalServerContext = createContext<LocalServerContextValue | null>(null);

export const useLocalServer = () => {
    const ctx = useContext(LocalServerContext);
    if (!ctx) throw new Error('useLocalServer must be used within LocalServerProvider');
    return ctx;
};
