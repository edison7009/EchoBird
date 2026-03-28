// Role APIs — CDN + cache-based role scanning
import { invoke } from '@tauri-apps/api/core';


export interface RoleCategory {
    id: string;
    name: string;
    label?: string; // alias for backward compat
    order?: number;
}

export interface RoleEntry {
    id: string;
    name: string;
    description: string;
    category: string;
    filePath: string;
    img?: string;
    fallbackImg?: string;
}

export interface RoleScanResult {
    categories: RoleCategory[];
    roles: RoleEntry[];
    locale: string;
    allLabel: string;
}

const ROLES_CDN_BASE = 'https://echobird.ai/roles';
const ROLES_CACHE_KEY = 'eb_roles_cache';

function resolveLocaleFileName(locale: string): string {
    // Chinese uses zh-Hans (special case)
    if (locale.startsWith('zh')) return 'roles-zh-Hans.json';
    // All other languages: derive from locale prefix (e.g. ja → roles-ja.json)
    // If the file doesn't exist on CDN, scanRoles() falls back to roles-en.json
    const prefix = locale.split('-')[0];
    if (prefix && prefix !== 'en') return `roles-${prefix}.json`;
    return 'roles-en.json';
}

async function fetchRolesFromCDN(fileName: string): Promise<{ categories: RoleCategory[]; roles: RoleEntry[] } | null> {
    try {
        const resp = await fetch(`${ROLES_CDN_BASE}/${fileName}`, { cache: 'no-cache' });
        if (!resp.ok) return null;
        return await resp.json();
    } catch {
        return null;
    }
}

export async function scanRoles(locale: string): Promise<RoleScanResult> {
    const localeFile = resolveLocaleFileName(locale);
    const enFile = 'roles-en.json';
    const cacheKey = `${ROLES_CACHE_KEY}_${localeFile}`;
    const isZh = locale.startsWith('zh');

    // Try user's language first
    let data = await fetchRolesFromCDN(localeFile);

    // Fallback to English if user's language failed and it's not already English
    if (!data && localeFile !== enFile) {
        data = await fetchRolesFromCDN(enFile);
    }

    // On success: cache to localStorage
    if (data) {
        try { localStorage.setItem(cacheKey, JSON.stringify(data)); } catch { /* quota exceeded */ }
    } else {
        // All CDN failed: try localStorage cache
        try {
            const cached = localStorage.getItem(cacheKey);
            if (cached) data = JSON.parse(cached);
        } catch { /* corrupted */ }
    }

    if (!data) {
        // Final fallback: empty result
        return { categories: [], roles: [], locale: isZh ? 'zh-Hans' : 'en', allLabel: isZh ? '\u5168\u90e8' : 'All' };
    }

    // Map category.name → label for backward compat
    const categories: RoleCategory[] = (data.categories || []).map((c: RoleCategory, i: number) => ({
        ...c,
        label: c.label || c.name,
        order: c.order ?? i,
    }));

    return {
        categories,
        roles: data.roles || [],
        locale: isZh ? 'zh-Hans' : 'en',
        allLabel: isZh ? '\u5168\u90e8' : 'All',
    };
}

export interface AgentStatus {
    id: string;
    name: string;
    installed: boolean;
    running?: boolean;
    path?: string;
}

export async function detectLocalAgents(): Promise<AgentStatus[]> {
    return invoke('detect_local_agents');
}
