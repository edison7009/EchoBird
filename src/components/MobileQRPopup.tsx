// MobileQRPopup — "Chat on the phone" QR code popup for Channels page
// Generates a QR code containing channel config so the mobile app can scan and sync.

import React, { useState, useRef, useEffect } from 'react';
import { QRCodeSVG } from 'qrcode.react';
import { Smartphone, X } from 'lucide-react';
import { useI18n } from '../hooks/useI18n';

interface MobileQRProps {
    /** Serialised config payload to encode in the QR code */
    payload: string;
}

export const MobileQRPopup: React.FC<MobileQRProps> = ({ payload }) => {
    const { t } = useI18n();
    const [open, setOpen] = useState(false);
    const popupRef = useRef<HTMLDivElement>(null);

    // Close on outside click
    useEffect(() => {
        if (!open) return;
        const handler = (e: MouseEvent) => {
            if (popupRef.current && !popupRef.current.contains(e.target as Node)) setOpen(false);
        };
        document.addEventListener('mousedown', handler);
        return () => document.removeEventListener('mousedown', handler);
    }, [open]);

    // Close on Escape
    useEffect(() => {
        if (!open) return;
        const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false); };
        document.addEventListener('keydown', handler);
        return () => document.removeEventListener('keydown', handler);
    }, [open]);

    return (
        <div className="relative" ref={popupRef}>
            {/* Trigger button */}
            <button
                onClick={() => setOpen(!open)}
                className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-mono text-cyber-accent/50 hover:text-cyber-accent hover:bg-cyber-accent/10 transition-all"
                title="Chat on the phone"
            >
                <Smartphone size={13} />
                <span className="hidden lg:inline">Chat on Phone</span>
            </button>

            {/* QR popup */}
            {open && (
                <div className="absolute top-full right-0 mt-2 z-50 animate-in fade-in slide-in-from-top-2 duration-200">
                    <div className="border border-cyber-accent/30 bg-cyber-bg rounded-xl shadow-lg shadow-cyber-accent/10 overflow-hidden"
                         style={{ minWidth: 260 }}>
                        {/* Header accent */}
                        <div className="h-[2px] w-full bg-gradient-to-r from-cyber-accent/0 via-cyber-accent/60 to-cyber-accent/0" />

                        {/* Close button */}
                        <button
                            onClick={() => setOpen(false)}
                            className="absolute top-2.5 right-2.5 p-1 rounded-md text-cyber-text-secondary/50 hover:text-cyber-accent hover:bg-cyber-accent/10 transition-colors"
                        >
                            <X size={12} />
                        </button>

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
