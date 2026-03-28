import React from 'react';
import * as api from '../../api/tauri';

// Markdown components config (Mother Agent blue theme)
export const mdComponents = {
    code: ({ className, children, ...props }: any) => {
        const isInline = !className;
        return isInline ? (
            <code className="bg-cyber-accent-secondary/10 text-cyber-accent-secondary px-1.5 py-0.5 rounded text-[0.85em] font-mono" {...props}>{children}</code>
        ) : (
            <code className={`block bg-black/40 border border-cyber-border/30 rounded-lg p-3 pr-10 my-2 text-[0.85em] font-mono text-cyber-text-primary overflow-x-auto whitespace-pre ${className || ''}`} {...props}>{children}</code>
        );
    },
    pre: ({ children }: any) => {
        const codeText = String(children?.props?.children || '').replace(/\n$/, '');
        const isBlock = children?.props?.className;
        if (!isBlock) return <>{children}</>;
        return (
            <div className="relative group">
                {children}
                <button
                    onClick={(e: React.MouseEvent<HTMLButtonElement>) => {
                        navigator.clipboard.writeText(codeText);
                        const btn = e.currentTarget;
                        const svg = btn.querySelector('svg');
                        if (svg) svg.style.display = 'none';
                        btn.insertAdjacentHTML('beforeend', '<span class="copy-ok" style="color:#7aa2f7">✓</span>');
                        setTimeout(() => {
                            const ok = btn.querySelector('.copy-ok');
                            if (ok) ok.remove();
                            if (svg) svg.style.display = '';
                        }, 1500);
                    }}
                    className="absolute top-2 right-2 w-6 h-6 flex items-center justify-center text-cyber-text-muted/40 hover:text-cyber-accent-secondary transition-colors"
                ><svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" /></svg></button>
            </div>
        );
    },
    a: ({ href, children }: any) => (
        <a href={href} className="text-cyber-accent-secondary hover:underline" onClick={(e: React.MouseEvent) => { e.preventDefault(); api.openExternal(href); }}>{children}</a>
    ),
    strong: ({ children }: any) => <strong className="text-cyber-text-primary font-bold">{children}</strong>,
    em: ({ children }: any) => <em className="text-cyber-accent-secondary/80">{children}</em>,
    ul: ({ children }: any) => <ul className="list-disc list-inside my-1 space-y-0.5">{children}</ul>,
    ol: ({ children }: any) => <ol className="list-decimal list-inside my-1 space-y-0.5">{children}</ol>,
    h1: ({ children }: any) => <h1 className="text-lg font-bold text-cyber-text-primary mt-3 mb-1">{children}</h1>,
    h2: ({ children }: any) => <h2 className="text-base font-bold text-cyber-text-primary mt-2 mb-1">{children}</h2>,
    h3: ({ children }: any) => <h3 className="text-sm font-bold text-cyber-text-primary mt-2 mb-1">{children}</h3>,
    blockquote: ({ children }: any) => <blockquote className="border-l-2 border-cyber-accent-secondary/40 pl-3 my-1 text-cyber-text-muted/60 italic">{children}</blockquote>,
    hr: () => <hr className="border-cyber-border/30 my-2" />,
    table: ({ children }: any) => <table className="border-collapse my-2 text-sm w-full">{children}</table>,
    th: ({ children }: any) => <th className="border border-cyber-border/30 px-2 py-1 bg-cyber-accent-secondary/5 text-left font-bold">{children}</th>,
    td: ({ children }: any) => <td className="border border-cyber-border/30 px-2 py-1">{children}</td>,
};
