# EchoBird App Detection And Provider Directory Design

**Goal:** Fix EchoBird's app-management misdetection for installed tools on this macOS machine and improve the right-panel provider directory so it is materially closer to the user's current CC Switch coverage.

**Scope:** This design covers four concrete problem areas: missing desktop-app version display, false "not installed" status for installed OpenClaw, missing detection for installed Codex/OpenClaw CLI binaries under ServBay's Node distribution, and incomplete static provider directory data. It does not include a dynamic import/sync feature from CC Switch's SQLite database.

**Non-goals:**
- Do not add direct runtime coupling to `~/.cc-switch/cc-switch.db`.
- Do not redesign the App Manager UI.
- Do not add a background sync daemon or watcher.
- Do not patch the installed `/Applications/EchoBird.app` bundle in place as the primary fix path.

**User-facing outcomes:**
- Codex Desktop, Cursor, and other supported desktop apps should show a concrete version instead of `-` when a version is available from the app bundle.
- OpenClaw should be shown as installed when the desktop app exists at `/Applications/OpenClaw.app`, even if the user is relying on the desktop app path rather than CLI-only detection.
- Codex CLI and OpenClaw CLI should be detected on this machine when installed under `/Applications/ServBay/package/node/current/bin/`.
- The right-panel provider directory should include a broader set of mainstream OpenAI-compatible and Anthropic-compatible channels so it no longer feels obviously weaker than the user's current CC Switch catalog.

**Architecture:**
EchoBird already has the right separation of responsibilities: `tools/*/paths.json` defines candidate install paths, `src-tauri/src/services/tool_manager.rs` turns those rules into `DetectedTool` entries, and `src/data/modelDirectory.json` drives the static provider directory shown in Model Nexus. The least disruptive fix is to keep those boundaries intact and improve each layer in place.

For detection, we will expand the macOS candidate paths for the affected tools so EchoBird can recognize the user's actual local install layout, including ServBay-managed global Node binaries and the OpenClaw desktop app bundle. For version display, we will add a desktop-app version path in the Rust backend that reads `CFBundleShortVersionString` or `CFBundleVersion` from a macOS app bundle instead of trying to run GUI apps with `--version`.

For the provider directory, we will continue using the existing static JSON directory instead of introducing dynamic provider introspection. The directory will be expanded with additional providers and more complete metadata, and App Manager's right panel will be taught to surface those bundled directory candidates alongside the user's already-saved models. This keeps the UX materially closer to CC Switch without crossing into live CC Switch database integration.

**Files and responsibilities:**
- `src-tauri/src/services/tool_manager.rs`
  Detect tools, choose the installed path, and assign version/model metadata for App Manager cards.
- `src-tauri/src/utils/platform.rs`
  Provide portable version helpers; add macOS bundle-version support here or in a closely related helper.
- `src-tauri/src/models/tool.rs`
  Extend `PathsConfig` only if a small new field is needed for desktop-version probing.
- `tools/codex/paths.json`
  Add missing CLI candidate paths for ServBay-managed installs.
- `tools/openclaw/paths.json`
  Add missing CLI candidate paths and desktop-app bundle candidate paths for macOS.
- `tools/cursor/paths.json`
  Keep current app path, but allow the backend to derive version from the app bundle.
- `tools/codexdesktop/paths.json`
  Keep current app path, but allow the backend to derive version from the app bundle.
- `src/data/modelDirectory.json`
  Expand the provider catalog and normalize metadata for added channels.
- `src/pages/AppManager/AppManagerComponents.tsx`
  Teach the right-side model selection panel to show bundled provider-directory candidates in addition to saved user models and official restore entries.
- `src/pages/AppManager/AppManagerProvider.tsx`
  Convert a bundled provider-directory pick into a concrete saved model entry, or otherwise bridge the panel selection state to the existing apply flow.

**Detailed design**

## 1. Desktop-app version detection

Current behavior in `scan_single_tool()` skips version probing whenever a tool is marked `noModelConfig`, which matches desktop launch-only tools. That avoids accidentally launching GUI apps with `--version`, but it also guarantees `version = None` unless a literal static version is hardcoded in `paths.json`. This is why Codex Desktop and likely Cursor render `版本: -` even though the app bundle version is locally available.

The fix is to add a desktop-safe version path:
- If the resolved installed path is inside a macOS `.app` bundle, read the bundle metadata instead of invoking the executable.
- Prefer `CFBundleShortVersionString`.
- Fall back to `CFBundleVersion`.
- Only use CLI `--version` probing for command-driven tools.

This should happen in the backend after install detection has already resolved the effective path, so the version logic uses the real detected path rather than guessing from the tool id.

## 2. OpenClaw installation detection

Current `tools/openclaw/paths.json` on macOS lists only CLI candidate paths such as `/opt/homebrew/bin/openclaw` and `~/.local/bin/openclaw`. That misses the user's installed `/Applications/OpenClaw.app`, so EchoBird treats the app as not installed and keeps showing the AI auto-install CTA.

We will add `/Applications/OpenClaw.app/Contents/MacOS/OpenClaw` as a macOS candidate path. This makes desktop-only installs detectable without special-casing OpenClaw in Rust. The tool should be considered installed if either the desktop app bundle or the CLI binary exists.

## 3. ServBay Node global binary detection

The user's installed CLI binaries resolve to:
- `/Applications/ServBay/package/node/current/bin/openclaw`
- `/Applications/ServBay/package/node/current/bin/codex`

EchoBird currently relies on `command_exists()` plus a login-shell fallback, then falls back to hardcoded path lists in `paths.json`. In practice, the GUI environment is not reliably surfacing these tools to EchoBird's scan, and the hardcoded path lists do not include the ServBay bin directory.

We will explicitly add the ServBay paths to the macOS path arrays for:
- `tools/openclaw/paths.json`
- `tools/codex/paths.json`

This reduces dependence on shell/PATH inheritance and makes the scan deterministic for this common local setup.

## 4. Provider directory expansion and App Manager exposure

`src/data/modelDirectory.json` currently contains 22 providers. That is a reasonable starter catalog, but it is visibly weaker than the user's CC Switch mental baseline. More importantly, App Manager's right panel does not currently read this directory at all — it only renders the user's already-saved `userModels` plus a single official restore card. That is why the screenshot shows just `OpenAI Official` instead of a fuller channel list.

The right goal for this change is not perfect parity with an evolving external catalog, but a broader first-party bundled list that covers the most commonly used providers and relay ecosystems, then exposing those entries directly in the App Manager panel. We will:
- expand the directory with additional entries where EchoBird can offer real value,
- show those entries in the App Manager right panel as selectable presets,
- keep the current "saved user model" flow intact,
- avoid any runtime dependence on CC Switch state.

The bundled candidates should behave like a quick-add source of truth:
- selecting a bundled provider candidate should prefill or create a normal EchoBird model entry,
- once saved, it should flow through the existing `applyModelToTool()` logic,
- official restore behavior remains a distinct first item where it already applies.

We will expand the directory with additional entries where EchoBird can offer real value:
- Mainstream direct providers not yet listed or not fully represented.
- Popular relay ecosystems and OpenAI-compatible gateways.
- China-accessible providers the user is likely to care about.

Each entry should include:
- `name`
- `url`
- `baseUrl` and/or `anthropicUrl`
- `modelId` when there is a sensible default
- `region`

We will avoid speculative or low-confidence entries. If a provider has multiple commonly used endpoints, prefer the stable public API base over undocumented internal routes.

**Error handling and fallback behavior:**
- If desktop bundle version parsing fails, leave `version` as `None`; never block install detection on version parsing.
- If multiple candidate install paths exist, keep the first successful match according to the configured path order.
- If a provider entry lacks a confident default model id, leave `modelId` empty rather than inventing one.

**Testing and verification strategy:**
- Validate that `scan_tools` returns non-empty `version` for at least Codex Desktop on macOS.
- Validate that `scan_tools` marks OpenClaw installed when `/Applications/OpenClaw.app` exists.
- Validate that `scan_tools` marks Codex CLI and OpenClaw CLI installed when the ServBay binary paths exist.
- Validate that the provider directory count increases and that added entries are visible in the right-panel source data.

**Risks:**
- Hardcoding ServBay-specific paths is environment-specific. This is acceptable here because the user explicitly runs that setup, but the path additions should be additive rather than replacing existing generic paths.
- Some desktop apps may not expose `CFBundleShortVersionString`; fallback logic must tolerate that.
- Provider directory expansion is static data maintenance; it improves coverage now but does not solve long-term synchronization with CC Switch.
