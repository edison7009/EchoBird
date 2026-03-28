// context.ts — Channels shared context
import React, { createContext, useContext } from 'react';

// ===== Types =====
export interface Attachment {
    type: 'image' | 'file';
    name: string;
    data: string; // base64 data URL (image) or text content (file)
    preview?: string; // Image thumbnail URL
}

export interface Channel {
    id: number;
    name: string;
    address: string;
    protocol: string;
    serverId?: string; // SSH server id — used to persist alias changes
}

// ===== Context =====
export interface ChannelsCtx {
    channels: Channel[];
    setChannels: React.Dispatch<React.SetStateAction<Channel[]>>;
    activeId: number | null;
    setActiveId: React.Dispatch<React.SetStateAction<number | null>>;
    selectChannel: (id: number) => void;
    allBridgeStatus: Record<number, string>;
    setAllBridgeStatus: React.Dispatch<React.SetStateAction<Record<number, string>>>;
    allActiveAgents: Record<number, string>;
    setAllActiveAgents: React.Dispatch<React.SetStateAction<Record<number, string>>>;
    allBridgeLoading: Record<number, boolean>;
    setAllBridgeLoading: React.Dispatch<React.SetStateAction<Record<number, boolean>>>;
    allSelectedRoles: Record<number, { id: string; name: string; filePath: string }>;
    setAllSelectedRoles: React.Dispatch<React.SetStateAction<Record<number, { id: string; name: string; filePath: string }>>>;
    allBridgeHasNew: Record<number, boolean>;
    setAllBridgeHasNew: React.Dispatch<React.SetStateAction<Record<number, boolean>>>;
}

export const ChannelsContext = createContext<ChannelsCtx | null>(null);
export const useChannels = () => useContext(ChannelsContext)!;
