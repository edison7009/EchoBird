/** @type {import('tailwindcss').Config} */
export default {
    content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
    theme: {
        extend: {
            colors: {
                'cyber-bg': '#0F1117',
                'cyber-terminal': '#131620',
                'cyber-surface': '#1A1F2B',
                'cyber-elevated': '#252B3D',
                'cyber-border': '#2D3448',
                'cyber-dark': '#0C0E14',
                'cyber-accent': '#00FF9D',
                'cyber-accent-secondary': '#00D4FF',
                'cyber-warning': '#facc15',
                'cyber-error': '#FFD700',
                'cyber-text': '#E0F7FA',
                'cyber-text-secondary': '#E2E8F0',
                'cyber-text-muted': '#A8B5C8',
            },
            fontFamily: {
                mono: ['JetBrains Mono', 'ui-monospace', 'monospace'],
                sans: ['Inter', 'system-ui', 'sans-serif'],
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
