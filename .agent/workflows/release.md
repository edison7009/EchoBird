---
description: Release a new version of Echobird (bump version, tag, push — GitHub Actions builds automatically)
---

# Release Workflow

Echobird uses a dual-repo architecture:
- **Private repo** `edison7009/Echobird` — source code + CI builds
- **Public repo** `edison7009/Echobird-MotherAgent` — release binaries + website (Cloudflare Pages)

The website's version API (`/api/version/index.json`) is hosted on the public repo and read by the app for update detection. **Both repos must be updated on every release.**

---

## Step 1: Bump version numbers

Update the version in **4 places**:

// turbo

1. `package.json` → `"version": "X.Y.Z"`
2. `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
3. `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
4. `docs/api/version/index.json` → `"version": "X.Y.Z"` (also update `releaseDate` and `releaseNotes`)

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

GitHub Actions will automatically:
- Build for Windows, macOS (arm64 + x86_64), Linux
- Upload artifacts to `Echobird-MotherAgent` as a **Draft Release**

Check build progress: https://github.com/edison7009/Echobird/actions

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
