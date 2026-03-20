---
description: Release a new version of Echobird (bump version, tag, push — GitHub Actions builds automatically)
---

# Release Workflow

Echobird uses a dual-repo architecture:
- **Private repo** `edison7009/Echobird` — source code only
- **Public repo** `edison7009/Echobird-MotherAgent` — CI builds (free!), release binaries, website (Cloudflare Pages)

**Build flow:** Private repo push tag → triggers public repo → public repo checks out private code → builds → publishes release (free Actions minutes!)

The website's version API (`/api/version/index.json`) is hosted on the public repo and read by the app for update detection.

> [!IMPORTANT]
> **Update `docs/api/version/index.json` ONLY AFTER the release is published** — not before.
> Updating it early causes users to see the update prompt while download links are still building (~10 min), resulting in broken downloads.

---

## Step 1: Bump version numbers

Update the version in **4 places** (NOT `docs/api/version/index.json` yet):

// turbo

1. `package.json` → `"version": "X.Y.Z"`
2. `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
3. `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
4. `bridge-src/Cargo.toml` → `version = "X.Y.Z"` (**MUST match main version** — remote auto-deploy uses this for version checking)

> [!CAUTION]
> **Always use `[System.IO.File]::WriteAllText(path, content, [System.Text.UTF8Encoding]::new($false))` to write files.** Never use `Set-Content -Encoding UTF8` — it adds a BOM that breaks `tauri-action` (JSON parse error).

---

## Step 2: Commit and push to private repo

```powershell
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml bridge-src/Cargo.toml
git commit -m "chore: bump version to vX.Y.Z"
git push origin main
```

---

## Step 3: Tag and trigger CI

```powershell
git tag vX.Y.Z
git push origin vX.Y.Z
```

This triggers the chain:
1. **Private repo** `release.yml` → sends `repository_dispatch` to public repo (takes seconds, almost zero cost)
2. **Public repo** `release.yml` → checks out private repo code → builds for Windows, macOS (arm64 + x86_64), Linux → uploads as **Draft Release** (100% free!)

⏳ **Wait for CI to finish before proceeding.** Check build progress: https://github.com/edison7009/Echobird-MotherAgent/actions

> [!IMPORTANT]
> **If `bridge-src/` was modified in this release:** After CI completes, download the 5 bridge binaries from the release artifacts and copy them to `bridge/`. This ensures subsequent releases (if bridge source is unchanged) ship the latest binaries for local + offline LAN deployment.

> [!CAUTION]
> **Never re-tag on CI failure.** If CI fails, bump to the next patch version (e.g. 2.6.0 → 2.6.1) and release fresh. Re-tagging causes CI to build from a stale commit, resulting in mismatched file names (e.g. title says v2.6.0 but files show 2.5.9).

---

## Step 4: Publish the release

Once all build jobs complete:

1. Go to https://github.com/edison7009/Echobird-MotherAgent/releases
2. Find the Draft Release for the new version
3. Edit the release notes if needed
4. Click **Publish release**

---

## Step 5: Update version API + sync docs

**Only do this AFTER the release is published** — this is what triggers the in-app update notification for existing users.

// turbo
```powershell
# 1. Update docs/api/version/index.json with new version, releaseDate, releaseNotes
# Use UTF-8 no-BOM write:
$content = '{"version": "X.Y.Z","releaseDate": "YYYY-MM-DD","releaseNotes": "..."}'
[System.IO.File]::WriteAllText("d:\Echobird\docs\api\version\index.json", $content, [System.Text.UTF8Encoding]::new($false))

# 2. Commit to private repo
git add docs/api/version/index.json
git commit -m "docs: publish vX.Y.Z release notes"
git push origin main

# 3. Sync docs to public repo
Copy-Item -Path "d:\Echobird\docs\*" -Destination "d:\Echobird-MotherAgent\docs\" -Recurse -Force
git -C "d:\Echobird-MotherAgent" add -A
git -C "d:\Echobird-MotherAgent" commit -m "docs: sync from private repo - vX.Y.Z"
git -C "d:\Echobird-MotherAgent" pull --rebase origin main
git -C "d:\Echobird-MotherAgent" push origin main
```

Cloudflare Pages redeploys automatically within 1-2 minutes. Users will now see the update notification.

---

## Secrets setup (one-time)

| Repo | Secret | Purpose |
|---|---|---|
| **Echobird** (private) | `RELEASE_TOKEN` | PAT with `repo` scope — used to trigger public repo dispatch |
| **Echobird-MotherAgent** (public) | `PRIVATE_REPO_TOKEN` | PAT with `repo` scope — used to checkout private repo code for building |

Both can use the same Personal Access Token if it has `repo` scope.
