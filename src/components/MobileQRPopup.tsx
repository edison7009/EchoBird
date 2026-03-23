// MobileQRPopup — "Chat on the phone" QR code popup for Channels page
// Generates a QR code containing channel config so the mobile app can scan and sync.

import React, { useState, useRef } from 'react';
import { QRCodeSVG } from 'qrcode.react';
import { Smartphone } from 'lucide-react';
import { useI18n } from '../hooks/useI18n';

interface MobileQRProps {
    /** Serialised config payload to encode in the QR code */
    payload: string;
}

export const MobileQRPopup: React.FC<MobileQRProps> = ({ payload }) => {
    const { t } = useI18n();
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
            {/* Trigger — hover only, no click, no highlight */}
            <span className="flex items-center gap-1.5 px-2.5 py-1 text-xs font-mono text-cyber-accent/50 cursor-default select-none">
                <Smartphone size={13} />
                <span className="hidden lg:inline">Chat on Phone</span>
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
                            {/* QR code with Echobird-style border */}
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

                            {/* Label */}
                            <div className="text-center">
                                <p className="text-xs font-mono text-cyber-accent font-medium tracking-wide">
                                    Scan with Echobird Mobile
                                </p>
                                <p className="text-[10px] font-mono text-cyber-text-secondary/50 mt-1">
                                    Sync channels, models & roles
                                </p>
                            </div>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
};
