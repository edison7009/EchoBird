/**
 * buildPendingMessage — shared utility for both MotherAgent and Channels pages.
 *
 * Takes pending attachments (files/images, models, skills) and produces:
 *  - `messageText`: the full text to send to the agent (includes base64 image data, model configs, skill installs)
 *  - `chips`: compact readonly chip descriptors to display below the user bubble
 */

import type { BubbleChip } from '../components/chat/ChatBubble';

export interface PendingFile {
    id: string;
    name: string;
    type: 'file' | 'image';
    preview?: string;   // base64 data URL for images
}

export interface PendingModel {
    id: string;
    name: string;
    modelId?: string;
    /** Raw ModelConfig fields needed for message text */
    baseUrl?: string;
    anthropicUrl?: string;
    apiKey?: string;
    proxyUrl?: string;
}

export interface PendingSkill {
    id: string;
    name: string;
    github: string;
    branch?: string;
}

export interface BuildResult {
    /** Full message text to send to agent */
    messageText: string;
    /** Chips to display below the user bubble */
    chips: BubbleChip[];
}

export function buildPendingMessage(
    userInput: string,
    files: PendingFile[],
    models: PendingModel[],
    skills: PendingSkill[],
): BuildResult {
    const parts: string[] = [];
    if (userInput.trim()) parts.push(userInput.trim());

    const chips: BubbleChip[] = [];

    // ── Files & images ──────────────────────────────────────────────────────
    if (files.length > 0) {
        const fileLines = files.map(f => {
            if (f.type === 'image' && f.preview) {
                return `- [Image: ${f.name}]\n  data: ${f.preview}`;
            }
            return `- [File: ${f.name}]`;
        }).join('\n');
        parts.push(`[Attached files]\n${fileLines}`);

        for (const f of files) {
            chips.push({
                type: f.type === 'image' ? 'image' : 'file',
                name: f.name,
                preview: f.preview,
            });
        }
    }

    // ── Models ──────────────────────────────────────────────────────────────
    if (models.length > 0) {
        const modelInfo = models.map(pm => {
            const lines: string[] = ['[MODEL CONFIG]', `Name: ${pm.name}`];
            if (pm.modelId) lines.push(`Model: ${pm.modelId}`);
            if (pm.baseUrl) lines.push(`Base URL: ${pm.baseUrl}`);
            if (pm.anthropicUrl) lines.push(`Anthropic URL: ${pm.anthropicUrl}`);
            if (pm.apiKey) lines.push(`API Key: ${pm.apiKey}`);
            if (pm.proxyUrl) lines.push(`Proxy URL: ${pm.proxyUrl}`);
            lines.push('[/MODEL CONFIG]');
            return lines.join('\n');
        }).join('\n\n');
        parts.push(modelInfo);

        for (const pm of models) {
            chips.push({ type: 'model', name: pm.name, modelId: pm.modelId });
        }
    }

    // ── Skills ──────────────────────────────────────────────────────────────
    if (skills.length > 0) {
        const skillInfo = skills.map(s => {
            const ownerRepo = s.github.split('/').slice(0, 2).join('/');
            const branch = s.branch || 'main';
            return `- ${s.name}\n  Install: \`openclaw skill install ${ownerRepo}@${branch}\``;
        }).join('\n');
        parts.push(`[Attached skills]\n${skillInfo}`);

        for (const s of skills) {
            chips.push({ type: 'skill', name: s.name });
        }
    }

    return {
        messageText: parts.join('\n\n'),
        chips,
    };
}
