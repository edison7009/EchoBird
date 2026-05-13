# EchoBird App Detection And Provider Directory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix EchoBird's macOS app detection/version display issues for Codex Desktop, Codex CLI, OpenClaw, Cursor, and ClaudeCode, and make App Manager expose a broader bundled provider list instead of only saved models plus the official restore entry.

**Architecture:** Keep EchoBird's existing separation: `tools/*/paths.json` remains the source of install-path truth, `tool_manager.rs` remains responsible for detection and version derivation, and `modelDirectory.json` remains the bundled provider catalog. Add a desktop-safe macOS bundle-version path instead of abusing CLI `--version` for GUI apps, expand path candidates for the user's real local layout, and wire App Manager so bundled provider presets can be selected alongside saved models.

**Tech Stack:** Rust (Tauri backend), TypeScript/React frontend data layer, JSON tool manifests, JSON provider directory.

---

### Task 1: Add macOS bundle-version detection

**Files:**
- Modify: `/tmp/EchoBird-edison7009/src-tauri/src/services/tool_manager.rs`
- Modify: `/tmp/EchoBird-edison7009/src-tauri/src/utils/platform.rs`

- [ ] **Step 1: Inspect the existing version logic and confirm the current guard**

Run: `sed -n '752,806p' /tmp/EchoBird-edison7009/src-tauri/src/services/tool_manager.rs`
Expected: the scan path shows `if version.is_none() && !pc.command.is_empty() && !pc.no_model_config { version = platform::get_version(&pc.command).await; }`

- [ ] **Step 2: Add a helper to read a macOS app bundle version from Info.plist**

Add a helper in `platform.rs` that takes an executable path and, on macOS, walks up to `Contents/Info.plist`, then reads `CFBundleShortVersionString` or `CFBundleVersion`.

```rust
#[cfg(target_os = "macos")]
pub fn get_macos_bundle_version(exe_path: &std::path::Path) -> Option<String> {
    let mut current = exe_path.to_path_buf();
    loop {
        let file_name = current.file_name()?.to_string_lossy();
        if file_name == "MacOS" {
            let contents_dir = current.parent()?;
            let plist_path = contents_dir.join("Info.plist");
            if !plist_path.exists() {
                return None;
            }

            let value = plist::Value::from_file(&plist_path).ok()?;
            let dict = value.as_dictionary()?;

            if let Some(v) = dict
                .get("CFBundleShortVersionString")
                .and_then(|v| v.as_string())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                return Some(v.to_string());
            }

            return dict
                .get("CFBundleVersion")
                .and_then(|v| v.as_string())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }

        if !current.pop() {
            return None;
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_macos_bundle_version(_exe_path: &std::path::Path) -> Option<String> {
    None
}
```

- [ ] **Step 3: Add the missing Rust dependency if it is not already present**

If `plist` is not already in `src-tauri/Cargo.toml`, add it under `[dependencies]`.

```toml
plist = "1"
```

- [ ] **Step 4: Update `scan_single_tool()` to use the detected path for desktop versions**

Replace the current version branch with logic that prefers macOS bundle parsing for detected desktop app paths and falls back to CLI `--version` for command-driven tools.

```rust
let mut version = pc.version.clone();

if installed {
    if let Some(sp) = find_skills_path(pc).await {
        skills_count = count_skills(&sp);
        skills_path_str = Some(sp);
    }
    if skills_path_str.is_none() {
        if let Some(ref rel_path) = pc.default_skills_path {
            let default_path = PathBuf::from(&def.tool_dir).join(rel_path);
            if default_path.exists() {
                let p = default_path.to_string_lossy().to_string();
                skills_count = count_skills(&p);
                skills_path_str = Some(p);
            }
        }
    }

    if version.is_none() {
        if let Some(ref path) = installed_path {
            let detected_path = Path::new(path);
            version = platform::get_macos_bundle_version(detected_path);
        }
    }

    if version.is_none() && !pc.command.is_empty() {
        version = platform::get_version(&pc.command).await;
    }
}
```

- [ ] **Step 5: Run formatting and a focused compile check**

Run:
- `cargo fmt --manifest-path /tmp/EchoBird-edison7009/src-tauri/Cargo.toml`
- `cargo check --manifest-path /tmp/EchoBird-edison7009/src-tauri/Cargo.toml`

Expected: format succeeds and `cargo check` passes.

### Task 2: Expand tool path rules for OpenClaw and Codex

**Files:**
- Modify: `/tmp/EchoBird-edison7009/tools/openclaw/paths.json`
- Modify: `/tmp/EchoBird-edison7009/tools/codex/paths.json`

- [ ] **Step 1: Add the user's ServBay CLI paths to Codex**

Update the macOS candidate path list in `tools/codex/paths.json` to include the ServBay Node bin path near the front of the list.

```json
"darwin": [
  "/Applications/ServBay/package/node/current/bin/codex",
  "/usr/local/bin/codex",
  "/opt/homebrew/bin/codex",
  "~/.bun/bin/codex",
  "~/.local/bin/codex",
  "~/.npm-global/bin/codex",
  "~/Library/pnpm/codex"
]
```

- [ ] **Step 2: Add both desktop and ServBay CLI paths to OpenClaw**

Update the macOS candidate path list in `tools/openclaw/paths.json`.

```json
"darwin": [
  "/Applications/OpenClaw.app/Contents/MacOS/OpenClaw",
  "/Applications/ServBay/package/node/current/bin/openclaw",
  "/usr/local/bin/openclaw",
  "/opt/homebrew/bin/openclaw",
  "~/Library/pnpm/openclaw",
  "~/.local/bin/openclaw",
  "~/.bun/bin/openclaw",
  "~/.npm-global/bin/openclaw"
]
```

- [ ] **Step 3: Validate the JSON files**

Run:
- `jq . /tmp/EchoBird-edison7009/tools/codex/paths.json >/dev/null`
- `jq . /tmp/EchoBird-edison7009/tools/openclaw/paths.json >/dev/null`

Expected: both commands exit successfully with no output.

### Task 3: Expand the bundled provider directory and expose it in App Manager

**Files:**
- Modify: `/tmp/EchoBird-edison7009/src/data/modelDirectory.json`
- Modify: `/tmp/EchoBird-edison7009/src/pages/AppManager/AppManagerComponents.tsx`
- Modify: `/tmp/EchoBird-edison7009/src/pages/AppManager/AppManagerProvider.tsx`

- [ ] **Step 1: Review the current directory and identify missing high-value entries**

Run: `python3 - <<'PY'\nimport json\nobj=json.load(open('/tmp/EchoBird-edison7009/src/data/modelDirectory.json'))\nprint(len(obj['providers']))\nfor item in obj['providers']:\n    print(item['name'])\nPY`

Expected: a provider count around the current 22 entries and a list missing several common relay-style channels.

- [ ] **Step 2: Add additional providers with concrete metadata**

Extend the `providers` array with additional static entries that EchoBird can safely prefill. Keep the same schema as existing entries.

```json
{
  "name": "OpenRouter",
  "url": "https://openrouter.ai",
  "baseUrl": "https://openrouter.ai/api/v1",
  "anthropicUrl": "",
  "modelId": "openai/gpt-4o-mini",
  "region": "global"
},
{
  "name": "SiliconFlow 硅基流动",
  "url": "https://siliconflow.cn",
  "baseUrl": "https://api.siliconflow.cn/v1",
  "anthropicUrl": "",
  "modelId": "",
  "region": "cn"
},
{
  "name": "One API",
  "url": "https://github.com/songquanpeng/one-api",
  "baseUrl": "",
  "anthropicUrl": "",
  "modelId": "",
  "region": "global"
},
{
  "name": "New API",
  "url": "https://github.com/QuantumNous/new-api",
  "baseUrl": "",
  "anthropicUrl": "",
  "modelId": "",
  "region": "global"
}
```

Use the same shape for any other added providers in this batch.

- [ ] **Step 3: Teach App Manager to render bundled provider presets**

Update App Manager so the right panel is not limited to `userModels`. The panel should render:
- local models,
- official restore entry when available,
- saved cloud models,
- bundled provider presets from `modelDirectory.json`.

The bundled preset card should display:
- provider name,
- normalized endpoint host/path,
- selection state consistent with the current card UI.

- [ ] **Step 4: Bridge bundled preset selection into the existing apply flow**

When the user selects a bundled provider preset for a tool, the provider should be materialized into the existing EchoBird model list before apply/launch runs. Reuse the current saved-model path instead of inventing a second apply pipeline.

The minimal acceptable behavior is:
- create or reuse a normal `ModelConfig` entry with the preset's `name`, `baseUrl`, `anthropicUrl`, and `modelId`,
- select that saved model's `internalId` in `toolModelConfig`,
- continue through the current `applyModelToTool()` code path.

- [ ] **Step 5: Validate the JSON file and TypeScript compile path**

Run: `jq . /tmp/EchoBird-edison7009/src/data/modelDirectory.json >/dev/null`

Expected: valid JSON.

Run: `npm --prefix /tmp/EchoBird-edison7009 run build`

Expected: frontend build passes with the new App Manager preset flow.

### Task 4: Verify detection behavior with the user's real local layout

**Files:**
- Modify: `/tmp/EchoBird-edison7009/src-tauri/src/services/tool_manager.rs` if any small follow-up fix is needed after verification

- [ ] **Step 1: Re-read the user's real installed paths and versions**

Run:
- `command -v openclaw && openclaw --version`
- `command -v codex && codex --version`
- `defaults read /Applications/EchoBird.app/Contents/Info CFBundleShortVersionString`

Expected: concrete local paths and version strings that match the reported machine state.

- [ ] **Step 2: Add temporary logging only if verification needs it**

If the first `cargo check` passes but detection is still ambiguous, add a short `log::info!` around the chosen `installed_path` / `version` in `scan_single_tool()`, verify, then remove it before finishing.

- [ ] **Step 3: Re-run compile check after any verification tweak**

Run: `cargo check --manifest-path /tmp/EchoBird-edison7009/src-tauri/Cargo.toml`

Expected: pass.

### Task 5: Commit the changes

**Files:**
- Add: `/tmp/EchoBird-edison7009/docs/superpowers/specs/2026-05-13-echobird-app-detection-and-provider-directory-design.md`
- Add: `/tmp/EchoBird-edison7009/docs/superpowers/plans/2026-05-13-echobird-app-detection-and-provider-directory.md`
- Modify: `/tmp/EchoBird-edison7009/src-tauri/src/services/tool_manager.rs`
- Modify: `/tmp/EchoBird-edison7009/src-tauri/src/utils/platform.rs`
- Modify: `/tmp/EchoBird-edison7009/src-tauri/Cargo.toml` if needed
- Modify: `/tmp/EchoBird-edison7009/tools/openclaw/paths.json`
- Modify: `/tmp/EchoBird-edison7009/tools/codex/paths.json`
- Modify: `/tmp/EchoBird-edison7009/src/data/modelDirectory.json`
- Modify: `/tmp/EchoBird-edison7009/src/pages/AppManager/AppManagerComponents.tsx`
- Modify: `/tmp/EchoBird-edison7009/src/pages/AppManager/AppManagerProvider.tsx`

- [ ] **Step 1: Inspect the final diff**

Run: `git -C /tmp/EchoBird-edison7009 diff --stat`

Expected: only the intended files changed.

- [ ] **Step 2: Commit with a focused message**

Run:
```bash
git -C /tmp/EchoBird-edison7009 add \
  docs/superpowers/specs/2026-05-13-echobird-app-detection-and-provider-directory-design.md \
  docs/superpowers/plans/2026-05-13-echobird-app-detection-and-provider-directory.md \
  src-tauri/src/services/tool_manager.rs \
  src-tauri/src/utils/platform.rs \
  src-tauri/Cargo.toml \
  tools/openclaw/paths.json \
  tools/codex/paths.json \
  src/data/modelDirectory.json \
  src/pages/AppManager/AppManagerComponents.tsx \
  src/pages/AppManager/AppManagerProvider.tsx
git -C /tmp/EchoBird-edison7009 commit -m "fix: improve app detection and provider coverage"
```

Expected: a single commit containing the detection and provider-directory improvements.
