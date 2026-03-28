---
description: Automatically compile and download the Linux bridge binary using a designated remote Linux server
---

# Remote Bridge Compilation Workflow

This workflow automates the process of cross-compiling the `echobird-bridge` binary for Linux by utilizing a physical or remote Ubuntu/Debian server as a dedicated build agent. It bypasses the need to wait for GitHub Actions CI.

## Prerequisites
- A remote Linux server (Ubuntu/Debian recommended) with SSH access enabled.
- Node.js installed on your local Windows development machine.

---

## Step 1: Run the Automated Deployment Script

The script will automatically connecting via SFTP, pack the source code, install `build-essential` and Rust if missing, compile in release mode, and download the resulting binary (`bridge-linux-x86_64` or `bridge-linux-aarch64`) directly into your `bridge/` bundling directory.

**// turbo**
```powershell
node .agent/scripts/build-bridge-remote.js 192.168.10.39 34567 eben 669966
```

> **Parameters breakdown**:
> `node .agent/scripts/build-bridge-remote.js [Host IP] [Port] [Username] [Password]`
> *If you omit the parameters, the script defaults exactly to the parameters above (the primary build laptop).*

---

## Step 2: Verify the Binary

Check that the new binary has successfully arrived in the distribution folder:

**// turbo**
```powershell
Get-Item "d:\Echobird\bridge\bridge-linux-*" | Select-Object Name, Length, LastWriteTime
```

---

## Step 3: Bundle and Distribute

Since the binary is now sitting securely in `d:\Echobird\bridge\`, it will automatically be packaged into the installer the next time you run `npm run tauri build` or trigger a release. 

Absolutely no further manual steps are required.
