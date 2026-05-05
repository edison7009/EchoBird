/** @type {import('tailwindcss').Config} */
export default {
    content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
    theme: {
        extend: {
            colors: {
                // All theme tokens point at CSS variables so light/dark mode swaps work.
                // The original "cyber-*" names are preserved (used in 22 files) — only the
                // values shift. Opacity modifiers (`bg-cyber-accent/20`) keep working
                // because each var is stored as space-separated RGB channels.
                'cyber-bg':               'rgb(var(--bg-base-rgb) / <alpha-value>)',
                'cyber-terminal':         'rgb(var(--bg-terminal-rgb) / <alpha-value>)',
                'cyber-surface':          'rgb(var(--bg-surface-rgb) / <alpha-value>)',
                'cyber-elevated':         'rgb(var(--bg-elevated-rgb) / <alpha-value>)',
                'cyber-border':           'rgb(var(--border-rgb) / <alpha-value>)',
                'cyber-dark':             'rgb(var(--bg-deep-rgb) / <alpha-value>)',
                'cyber-accent':           'rgb(var(--accent-rgb) / <alpha-value>)',
                'cyber-accent-secondary': 'rgb(var(--accent-secondary-rgb) / <alpha-value>)',
                'cyber-warning':          'rgb(var(--warning-rgb) / <alpha-value>)',
                'cyber-error':            'rgb(var(--error-rgb) / <alpha-value>)',
                'cyber-text':             'rgb(var(--text-primary-rgb) / <alpha-value>)',
                'cyber-text-secondary':   'rgb(var(--text-secondary-rgb) / <alpha-value>)',
                'cyber-text-muted':       'rgb(var(--text-muted-rgb) / <alpha-value>)',
                'cyber-input':            'rgb(var(--bg-input-rgb) / <alpha-value>)',
            },
            fontFamily: {
                // Latin via Inter; CJK falls back to PingFang SC (macOS),
                // then embedded Noto Sans SC (Windows / Linux), then Microsoft YaHei.
                sans: [
                    'Inter', '-apple-system', 'BlinkMacSystemFont',
                    'PingFang SC', 'Noto Sans SC', 'Hiragino Sans GB',
                    'Microsoft YaHei', 'Segoe UI', 'Roboto', 'Helvetica',
                    'Arial', 'sans-serif',
                ],
                mono: [
                    'CascadiaMono', 'Cascadia Code', 'JetBrains Mono',
                    'SF Mono', 'Menlo', 'Monaco', 'Consolas',
                    'Courier New', 'ui-monospace', 'monospace',
                ],
            },
            borderRadius: {
                'card': '8px',
                'button': '6px',
            },
            boxShadow: {
                'cyber-glow': '0 0 15px rgba(0,0,0,0.3), 0 0 1px rgba(255,255,255,0.05)',
                'cyber-card': '0 2px 10px rgba(0,0,0,0.5), 0 0 1px rgba(255,255,255,0.04)',
            },
        },
    },
    plugins: [],
};
