// Synchronous Linux check — used to skip looping animations that pin the CPU
// on Linux WebView2/WebKitGTK. navigator.platform is deprecated but reliable
// inside the Tauri WebView; userAgentData.platform is the modern equivalent.
export const IS_LINUX: boolean = (() => {
    try {
        const ua = (navigator as unknown as { userAgentData?: { platform?: string } }).userAgentData;
        const platform = (ua?.platform || navigator.platform || '').toLowerCase();
        return platform.includes('linux');
    } catch {
        return false;
    }
})();
