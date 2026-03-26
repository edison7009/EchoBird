---
description: Build and install Echobird Android APK to a USB-connected device (arm64-v8a only, ~30MB)
---

# Android USB Install Workflow

Builds a debug-signed APK for arm64-v8a only (covers ~98% of Android devices) and installs it via USB.

> [!IMPORTANT]
> Only arm64-v8a is built. This keeps the APK ~30MB and build time fast.
> Do NOT use `assembleDebug` (builds 4 architectures = slow + large).
> Do NOT use `cargo tauri android build --apk` for installation (produces unsigned APK).

---

## Prerequisites

- USB Debugging enabled on device (Settings → Developer Options → USB Debugging)
- Phone connected via USB and authorized (tap "Allow" on device)
- Android SDK installed at `C:\Users\eben\AppData\Local\Android\Sdk`

---

## Step 1: Verify device is connected

// turbo
```powershell
& "C:\Users\eben\AppData\Local\Android\Sdk\platform-tools\adb.exe" devices
```

Expected output: a device ID followed by `device` (not `unauthorized`).

---

## Step 2: Build release APK (arm64 + all arches for universal)

Only Rust code compiles; Vite rebuilds frontend automatically. Rust is incremental — fast if source unchanged.

```powershell
$env:ANDROID_HOME = "C:\Users\eben\AppData\Local\Android\Sdk"
cargo tauri android build --apk 2>&1
```

Output APK:
`D:\Echobird\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release-unsigned.apk`

> [!IMPORTANT]
> This produces an **unsigned** APK — cannot install directly. Always follow Step 3.

---

## Step 3: Sign with debug keystore + install (one command)

// turbo
```powershell
$env:JAVA_HOME = "C:\Program Files\Android\Android Studio\jbr"
& "C:\Users\eben\AppData\Local\Android\Sdk\build-tools\36.1.0\apksigner.bat" sign `
    --ks "C:\Users\eben\.android\debug.keystore" `
    --ks-pass pass:android --key-pass pass:android `
    --ks-key-alias androiddebugkey `
    --out "D:\Echobird\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release-signed.apk" `
    "D:\Echobird\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release-unsigned.apk"

& "C:\Users\eben\AppData\Local\Android\Sdk\platform-tools\adb.exe" install -r `
    "D:\Echobird\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release-signed.apk"
```

Expected final line: `Success`

---

## Notes

- **CSS/JS only change**: ~2-3 min (Vite 3s + Gradle incremental, Rust skipped)
- **Rust code changed**: ~5 min (Rust incremental for changed crates only)
- ⚠️ `assembleArm64Debug` is NOT recommended — debug Rust profile is a completely separate compile from release and takes 5+ min even with cache
- If device shows `unauthorized`: unplug/replug and tap "Allow" on phone
- If you see `INSTALL_PARSE_FAILED_NO_CERTIFICATES`: you forgot Step 3 signing
