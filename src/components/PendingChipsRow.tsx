import { X, Paperclip } from 'lucide-react';

// ─── Types ──────────────────────────────────────────────────────────────────

export interface PendingFile {
    id: string;
    name: string;
    type: 'file' | 'image';
    preview?: string;
}

export interface PendingChipsRowProps {
    files: PendingFile[];
    onRemoveFile: (id: string) => void;
}

// ─── Shared chip base class ──────────────────────────────────────────────────
// All chips: h-7 (1.75rem = 28px), fixed height, font-mono, rounded, px-2

const BASE = 'flex items-center gap-1.5 h-7 rounded px-2 text-xs font-mono border';

const VARIANTS = {
    file:  `${BASE} bg-cyber-bg/80 border-cyber-text-muted/60 text-cyber-text-muted`,
} as const;

const REMOVE_BTN = 'ml-0.5 transition-colors hover:text-red-400';
const REMOVE_BTN_FILE  = `${REMOVE_BTN} text-cyber-text-muted/40`;

// ─── PendingChipsRow component ───────────────────────────────────────────────

export function PendingChipsRow({
    files, onRemoveFile,
}: PendingChipsRowProps) {
    if (files.length === 0) return null;

    return (
        <div className="flex flex-wrap gap-2 px-3 pt-2 pb-1 max-h-[4.5rem] overflow-y-auto custom-scrollbar">

            {/* ── File / image chips ── */}
            {files.map(f => (
                <div key={f.id} className={VARIANTS.file}>
                    {f.type === 'image' && f.preview
                        ? <img src={f.preview} alt={f.name} className="w-4 h-4 object-cover rounded flex-shrink-0" />
                        : <Paperclip size={12} className="text-cyber-accent-secondary/60 flex-shrink-0" />
                    }
                    <span className="max-w-[120px] truncate">{f.name}</span>
                    <button onClick={() => onRemoveFile(f.id)} className={REMOVE_BTN_FILE}>
                        <X size={11} />
                    </button>
                </div>
            ))}
        </div>
    );
}
