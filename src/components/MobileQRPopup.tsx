// MobileConfigPopup — hover popup for copying config code to sync with mobile app

import React, { useState, useRef } from 'react';
import { useI18n } from '../hooks/useI18n';
import { open as shellOpen } from '@tauri-apps/plugin-shell';

interface MobileConfigProps {
    /** Base64-encoded config string (eb:...) to copy */
    configCode: string;
}

// Phone icon — thin outline style
const PhoneIcon = () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <rect x="5" y="2" width="14" height="20" rx="2" ry="2" />
        <line x1="12" y1="18" x2="12" y2="18.01" strokeWidth="2.5" />
    </svg>
);

export const MobileQRPopup: React.FC<MobileConfigProps> = ({ configCode }) => {
    const [open, setOpen] = useState(false);
    const [copied, setCopied] = useState(false);
    const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const { t } = useI18n();

    const showPopup = () => {
        if (hideTimer.current) { clearTimeout(hideTimer.current); hideTimer.current = null; }
        setOpen(true);
    };
    const scheduleHide = () => {
        hideTimer.current = setTimeout(() => setOpen(false), 200);
    };

    const handleCopy = async () => {
        try {
            await navigator.clipboard.writeText(configCode);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch {
            const ta = document.createElement('textarea');
            ta.value = configCode;
            document.body.appendChild(ta);
            ta.select();
            document.execCommand('copy');
            document.body.removeChild(ta);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        }
    };

    return (
        <div className="relative" onMouseEnter={showPopup} onMouseLeave={scheduleHide}>
            {/* Trigger — icon only, hover to show popup */}
            <span className="flex items-center p-1.5 text-cyber-accent/40 cursor-default select-none">
                <PhoneIcon />
            </span>

            {/* Config sync popup */}
            {open && (
                <div className="absolute top-full right-0 mt-2 z-50 animate-in fade-in slide-in-from-top-2 duration-200">
                    <div className="border border-cyber-accent/30 bg-cyber-bg rounded-xl shadow-lg shadow-cyber-accent/10 overflow-hidden"
                         style={{ minWidth: 280 }}>
                        {/* Header accent */}
                        <div className="h-[2px] w-full bg-gradient-to-r from-cyber-accent/0 via-cyber-accent/60 to-cyber-accent/0" />

                        {/* Content */}
                        <div className="flex flex-col items-center px-5 pt-4 pb-4 gap-3">
                            {/* Copy button */}
                            <button
                                onClick={handleCopy}
                                className="w-full py-2.5 px-4 rounded-lg font-mono text-[13px] font-medium tracking-wide transition-all cursor-pointer border"
                                style={{
                                    background: copied ? 'rgba(0, 255, 157, 0.15)' : 'rgba(0, 255, 157, 0.08)',
                                    borderColor: copied ? 'rgba(0, 255, 157, 0.5)' : 'rgba(0, 255, 157, 0.25)',
                                    color: copied ? '#00ff9d' : 'rgba(0, 255, 157, 0.8)',
                                }}
                            >
                                {copied ? `✓ ${t('mobile.copied')}` : t('mobile.syncToPhone')}
                            </button>

                            {/* Instruction */}
                            <p className="text-[12px] font-mono text-cyber-text-secondary/50 text-center leading-relaxed">
                                {t('mobile.pasteInApp')}
                            </p>

                            {/* Download links */}
                            <div className="flex items-center justify-center gap-4">
                                <span onClick={() => shellOpen('https://echobird.ai/mobile').catch(() => window.open('https://echobird.ai/mobile', '_blank'))}
                                   className="text-[11px] font-mono text-cyber-text-secondary/40 hover:text-cyber-accent transition-colors cursor-pointer">
                                    Android
                                </span>
                                <span onClick={() => shellOpen('https://echobird.ai/mobile').catch(() => window.open('https://echobird.ai/mobile', '_blank'))}
                                   className="text-[11px] font-mono text-cyber-text-secondary/40 hover:text-cyber-accent transition-colors cursor-pointer">
                                    iOS
                                </span>
                            </div>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
};
