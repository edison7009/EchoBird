---
description: Release a new version of Echobird (bump version, tag, push — GitHub Actions builds automatically)
---

# Release Workflow

Echobird uses a dual-repo architecture:
- **Private repo** `edison7009/Echobird` — source code only
- **Public repo** `edison7009/Echobird-MotherAgent` — CI builds (free!), release binaries, website (Cloudflare Pages)

**Build flow:** Private repo push tag → triggers public repo → public repo checks out private code → builds → publishes release (free Actions minutes!)

The website's version API (`/api/version/index.json`) is hosted on the public repo and read by the app for update detection. **Both repos must be updated on every release.**

---

## Step 1: Bump version numbers

Update the version in **5 places**:

// turbo

1. `package.json` → `"version": "X.Y.Z"`
2. `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
3. `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
4. `plugins/llm-server/Cargo.toml` → `version = "X.Y.Z"` (keeps plugin version in sync — shown in binary `--version` output)
5. `docs/api/version/index.json` → `"version": "X.Y.Z"` (also update `releaseDate` and `releaseNotes`)

---

## Step 2: Commit and push to private repo

```powershell
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml docs/api/version/index.json
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

Check build progress: https://github.com/edison7009/Echobird-MotherAgent/actions

---

## Step 4: Sync docs to public repo

⚠️ **Required** — the app's update detection reads version from the public website.

// turbo
```powershell
Copy-Item -Path "d:\Echobird\docs\*" -Destination "d:\Echobird-MotherAgent\docs\" -Recurse -Force
git -C "d:\Echobird-MotherAgent" add -A
git -C "d:\Echobird-MotherAgent" commit -m "docs: sync from private repo — vX.Y.Z"
git -C "d:\Echobird-MotherAgent" push origin main
```

Cloudflare Pages redeploys automatically within 1-2 minutes.

---

## Step 5: Publish the release

Once all 4 build jobs complete:

1. Go to https://github.com/edison7009/Echobird-MotherAgent/releases
2. Find the Draft Release for the new version
3. Edit the release notes if needed
4. Click **Publish release**

Users can now download from the public repo Releases page.

---

## Secrets setup (one-time)

| Repo | Secret | Purpose |
|---|---|---|
| **Echobird** (private) | `RELEASE_TOKEN` | PAT with `repo` scope — used to trigger public repo dispatch |
| **Echobird-MotherAgent** (public) | `PRIVATE_REPO_TOKEN` | PAT with `repo` scope — used to checkout private repo code for building |

Both can use the same Personal Access Token if it has `repo` scope.
