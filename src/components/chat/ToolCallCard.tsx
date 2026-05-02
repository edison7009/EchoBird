// ToolCallCard — collapsible row that shows what tool the agent is using,
// its arguments, and the result. Modeled after Claude.ai / Cursor's
// "Calling tool…" panels: a one-line preview while running, expandable
// for full args + output.
import { useState } from 'react';
import {
    Loader2, ChevronRight, Check, X as XIcon,
    Terminal, Globe, FileText, FileEdit, Upload, Download,
    KeyRound, Wrench,
} from 'lucide-react';

export interface ToolCallCardProps {
    name: string;
    args: string;
    status: 'running' | 'done' | 'failed';
    output?: string;
}

// Map tool name → icon + a friendly label preview key
const TOOL_META: Record<string, { icon: any; label: string; previewKey?: string }> = {
    shell_exec:           { icon: Terminal,  label: 'shell',          previewKey: 'command' },
    ssh_shell:            { icon: Terminal,  label: 'ssh',            previewKey: 'command' },
    file_read:            { icon: FileText,  label: 'read',           previewKey: 'path' },
    file_write:           { icon: FileEdit,  label: 'write',          previewKey: 'path' },
    web_fetch:            { icon: Globe,     label: 'fetch',          previewKey: 'url' },
    upload_file:          { icon: Upload,    label: 'upload',         previewKey: 'remote_path' },
    download_file:        { icon: Download,  label: 'download',       previewKey: 'remote_path' },
    get_sudo_password:    { icon: KeyRound,  label: 'sudo',           previewKey: 'server_id' },
    deploy_plugin_source: { icon: Wrench,    label: 'deploy plugin',  previewKey: 'plugin_id' },
};

function previewFor(toolName: string, args: string): string {
    const meta = TOOL_META[toolName];
    if (!meta?.previewKey) return '';
    try {
        const obj = JSON.parse(args);
        const val = obj?.[meta.previewKey];
        if (typeof val === 'string') return val;
    } catch { /* args still streaming — no parseable JSON yet */ }
    return '';
}

const OUTPUT_PREVIEW_LIMIT = 4000;

export function ToolCallCard({ name, args, status, output }: ToolCallCardProps) {
    const [open, setOpen] = useState(false);

    const meta = TOOL_META[name] || { icon: Wrench, label: name };
    const Icon = meta.icon;
    const preview = previewFor(name, args);

    const statusIcon =
        status === 'running' ? <Loader2 size={11} className="animate-spin text-cyber-accent" />
        : status === 'failed' ? <XIcon size={11} className="text-red-400" />
        : <Check size={11} className="text-cyber-accent" />;

    const borderColor =
        status === 'failed' ? 'border-red-500/30'
        : status === 'running' ? 'border-cyber-accent/40'
        : 'border-cyber-border/40';

    const truncatedOutput = output && output.length > OUTPUT_PREVIEW_LIMIT
        ? output.slice(0, OUTPUT_PREVIEW_LIMIT) + '\n…\n[truncated]'
        : output;

    return (
        <div className={`mb-3 border ${borderColor} rounded-md bg-black/30 text-[12px] font-mono`}>
            <button
                type="button"
                onClick={() => setOpen(o => !o)}
                className="w-full flex items-center gap-2 px-2.5 py-1.5 text-left hover:bg-white/5 transition-colors rounded-md"
            >
                <ChevronRight size={11} className={`flex-shrink-0 text-cyber-text-muted/60 transition-transform ${open ? 'rotate-90' : ''}`} />
                <Icon size={12} className="flex-shrink-0 text-cyber-accent/80" />
                <span className="flex-shrink-0 text-cyber-accent/80 font-bold">{meta.label}</span>
                {preview && (
                    <span className="flex-1 truncate text-cyber-text-muted/70">
                        {preview}
                    </span>
                )}
                <span className="flex-shrink-0">{statusIcon}</span>
            </button>

            {open && (
                <div className="px-2.5 pb-2 pt-1 space-y-2 border-t border-cyber-border/30">
                    <div>
                        <div className="text-[10px] text-cyber-text-muted/60 mb-0.5">arguments</div>
                        <pre className="bg-cyber-bg/60 px-2 py-1 rounded text-cyber-text-primary whitespace-pre-wrap break-all">
                            {args || '<streaming…>'}
                        </pre>
                    </div>
                    {truncatedOutput !== undefined && (
                        <div>
                            <div className="text-[10px] text-cyber-text-muted/60 mb-0.5">output</div>
                            <pre className={`px-2 py-1 rounded whitespace-pre-wrap break-all max-h-72 overflow-y-auto ${status === 'failed' ? 'bg-red-950/30 text-red-300' : 'bg-cyber-bg/60 text-cyber-text-primary'}`}>
                                {truncatedOutput}
                            </pre>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}
