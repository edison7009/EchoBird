// Platform detection utility
// Returns true when running on Android/iOS (real device or emulator)

export function isMobile(): boolean {
    // 1. Most reliable: check userAgent (works on all real Android/iOS devices)
    const ua = navigator.userAgent || '';
    if (/Android/i.test(ua) || /iPhone|iPad|iPod/i.test(ua)) return true;
    // 2. Tauri 2 internals (backup)
    try {
        const w = window as any;
        const os = w.__TAURI_INTERNALS__?.metadata?.currentPlatform?.os?.name;
        if (os === 'android' || os === 'ios') return true;
    } catch { /* ignore */ }
    // 3. Dev fallback: ?mobile=1 in URL
    if (new URLSearchParams(window.location.search).has('mobile')) return true;
    return false;
}
