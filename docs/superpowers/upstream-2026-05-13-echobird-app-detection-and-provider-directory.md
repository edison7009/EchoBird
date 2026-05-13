# EchoBird Upstream Submission Notes

## Suggested PR Title

`fix: improve app detection, desktop version display, and App Manager provider coverage`

## Suggested Issue / PR Background

This change comes from real-world macOS usage where EchoBird was being evaluated as a replacement for CC Switch.

Observed problems in the App Manager UI:

1. **Missing version display**
   - `Codex Desktop` showed `版本: -`
   - `Cursor` showed `版本: -`
   - `ClaudeCode` showed `版本: -`

2. **False negative install detection**
   - `OpenClaw` was already installed and running locally, but EchoBird still showed the AI auto-install CTA.

3. **CLI detection gaps on macOS**
   - `codex` CLI was installed and available via:
     - `/Applications/ServBay/package/node/current/bin/codex`
   - `openclaw` CLI was installed and available via:
     - `/Applications/ServBay/package/node/current/bin/openclaw`
   - EchoBird did not detect those binaries in App Manager.

4. **App Manager right panel felt much weaker than expected**
   - The right-side model channel list was effectively limited to saved `userModels` plus `OpenAI Official`.
   - This made the panel feel significantly less useful than CC Switch, even though EchoBird already ships a bundled provider directory.

## Reproduction Context

Tested on macOS with these locally available apps / binaries:

- `/Applications/Codex.app`
- `/Applications/OpenClaw.app`
- `/Applications/Cursor.app`
- `codex` CLI at `/Applications/ServBay/package/node/current/bin/codex`
- `openclaw` CLI at `/Applications/ServBay/package/node/current/bin/openclaw`
- `claude --version` returning `2.1.97 (Claude Code)`

Bundle versions confirmed locally:

- `Codex.app`: `26.506.31421`
- `Cursor.app`: `3.3.30`
- `OpenClaw.app`: `2026.4.11`

## Root Cause Summary

### 1. Desktop app versions

`scan_single_tool()` only attempted CLI `--version` probing when `command` was present and `noModelConfig` was false.

That meant:
- desktop apps like `codexdesktop` never got a version unless it was hardcoded
- GUI-safe version metadata already present in macOS app bundles was ignored

### 2. OpenClaw install detection

`tools/openclaw/paths.json` on macOS only listed CLI paths.

It did **not** include:
- `/Applications/OpenClaw.app/Contents/MacOS/OpenClaw`

So desktop-app-only installs were treated as not installed.

### 3. Codex/OpenClaw CLI detection

The macOS path lists did not include ServBay's Node global bin path:

- `/Applications/ServBay/package/node/current/bin/codex`
- `/Applications/ServBay/package/node/current/bin/openclaw`

Even though EchoBird has PATH-based detection fallbacks, explicit path coverage is still needed for common GUI-launch environments where inherited PATH is incomplete or inconsistent.

### 4. App Manager provider coverage

The bundled provider directory already existed in `src/data/modelDirectory.json`, but App Manager's right panel did not use it.

It only rendered:
- local user-saved models
- an official restore entry when applicable

So the panel did not expose the bundled provider/relay catalog at all.

## What This PR Changes

### Backend detection and versions

- Add macOS bundle-version detection via `Info.plist`
  - prefer `CFBundleShortVersionString`
  - fall back to `CFBundleVersion`
- Keep CLI `--version` probing for command-driven tools
- Add a fallback `get_version_from_path()` path for already-detected executable paths

### Tool path coverage

- Add `/Applications/OpenClaw.app/Contents/MacOS/OpenClaw` to OpenClaw's macOS path candidates
- Add ServBay-managed CLI paths for:
  - `codex`
  - `openclaw`

### App Manager provider UX

- Expose bundled provider presets in the App Manager right panel
- Keep existing saved-model flow intact
- Selecting a bundled preset materializes it into a normal EchoBird model entry and then reuses the existing apply pipeline

### Bundled provider catalog

- Expand bundled provider and relay coverage with a few additional high-value entries:
  - `SiliconFlow 硅基流动`
  - `PPIO 派欧云`
  - `302.AI`
  - `OpenAI-Compatible Custom`
  - `One API`
  - `New API`
  - `LibreChat AI Gateway`

## Validation Performed

- `cargo check --manifest-path src-tauri/Cargo.toml` ✅
- Local path verification confirmed all target paths exist ✅
- Local bundle-version reads for Codex / Cursor / OpenClaw succeeded ✅
- App Manager-related TypeScript changes passed `tsc --noEmit` ✅

Notes on repository-level frontend verification:

- `npm run build` currently fails due an existing Tailwind/PostCSS configuration mismatch:
  - `It looks like you're trying to use tailwindcss directly as a PostCSS plugin...`
- This appears unrelated to the changes in this PR.
- Full-repo `lint` also exceeds the existing warning budget outside the changed scope.

## Scope Boundaries

This PR intentionally does **not**:

- add live synchronization with `~/.cc-switch/cc-switch.db`
- redesign App Manager UI layout
- add a background provider sync system

The goal is a scoped, upstream-safe improvement that fixes real detection bugs and surfaces the provider directory EchoBird already ships.
