import { createContext, useContext } from 'react';
import type { MotherAgentCtx } from './types';

export const MotherAgentContext = createContext<MotherAgentCtx | null>(null);

export const useMotherAgent = () => {
    const ctx = useContext(MotherAgentContext);
    if (!ctx) throw new Error('useMotherAgent must be used within MotherAgentProvider');
    return ctx;
};
