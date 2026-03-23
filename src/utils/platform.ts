// Platform detection utility
// Returns true when running inside Tauri on Android/iOS

export function isMobile(): boolean {
    // Tauri 2 exposes __TAURI_INTERNALS__ with platform info
    const w = window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentPlatform?: { os?: { name?: string } } } } };
    const os = w.__TAURI_INTERNALS__?.metadata?.currentPlatform?.os?.name;
    if (os === 'android' || os === 'ios') return true;
    // Fallback: check URL param for development (?mobile=1)
    if (new URLSearchParams(window.location.search).has('mobile')) return true;
    return false;
}
