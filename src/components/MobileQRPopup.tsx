// MobileQRPopup — "Chat on the phone" QR code popup for Channels page
// Generates a QR code containing channel config so the mobile app can scan and sync.

import React, { useState, useRef } from 'react';
import { QRCodeSVG } from 'qrcode.react';

interface MobileQRProps {
    /** Serialised config payload to encode in the QR code */
    payload: string;
}

// Phone icon — thin outline style that clearly reads as a mobile phone
const PhoneIcon = () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <rect x="5" y="2" width="14" height="20" rx="2" ry="2" />
        <line x1="12" y1="18" x2="12" y2="18.01" strokeWidth="2.5" />
    </svg>
);

export const MobileQRPopup: React.FC<MobileQRProps> = ({ payload }) => {
    const [open, setOpen] = useState(false);
    const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const containerRef = useRef<HTMLDivElement>(null);

    const showPopup = () => {
        if (hideTimer.current) { clearTimeout(hideTimer.current); hideTimer.current = null; }
        setOpen(true);
    };
    const scheduleHide = () => {
        hideTimer.current = setTimeout(() => setOpen(false), 200);
    };

    return (
        <div className="relative" ref={containerRef} onMouseEnter={showPopup} onMouseLeave={scheduleHide}>
            {/* Trigger — icon only, hover to show QR */}
            <span className="flex items-center p-1.5 text-cyber-accent/40 cursor-default select-none">
                <PhoneIcon />
            </span>

            {/* QR popup */}
            {open && (
                <div className="absolute top-full right-0 mt-2 z-50 animate-in fade-in slide-in-from-top-2 duration-200">
                    <div className="border border-cyber-accent/30 bg-cyber-bg rounded-xl shadow-lg shadow-cyber-accent/10 overflow-hidden"
                         style={{ minWidth: 260 }}>
                        {/* Header accent */}
                        <div className="h-[2px] w-full bg-gradient-to-r from-cyber-accent/0 via-cyber-accent/60 to-cyber-accent/0" />

                        {/* Content */}
                        <div className="flex flex-col items-center px-6 pt-4 pb-5 gap-3">
                            {/* QR code */}
                            <div className="p-3 rounded-lg bg-white">
                                <QRCodeSVG
                                    value={payload}
                                    size={180}
                                    level="M"
                                    bgColor="#ffffff"
                                    fgColor="#0d0f1a"
                                    imageSettings={{
                                        src: '/ico.svg',
                                        x: undefined,
                                        y: undefined,
                                        height: 28,
                                        width: 28,
                                        excavate: true,
                                    }}
                                />
                            </div>

                            {/* Label — 14px for readability */}
                            <div className="text-center">
                                <p className="text-[14px] font-mono text-cyber-accent font-medium tracking-wide">
                                    Scan with Echobird APP
                                </p>
                                <div className="flex items-center justify-center gap-3 mt-2">
                                    <a href="https://echobird.ai/download/android" target="_blank" rel="noopener noreferrer"
                                       className="text-[12px] font-mono text-cyber-text-secondary/60 hover:text-cyber-accent transition-colors underline underline-offset-2 decoration-cyber-accent/20">
                                        Android
                                    </a>
                                    <span className="text-cyber-text-secondary/20">|</span>
                                    <a href="https://echobird.ai/download/ios" target="_blank" rel="noopener noreferrer"
                                       className="text-[12px] font-mono text-cyber-text-secondary/60 hover:text-cyber-accent transition-colors underline underline-offset-2 decoration-cyber-accent/20">
                                        iOS
                                    </a>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
};
