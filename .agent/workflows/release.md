---
description: Release a new version of Echobird (bump version, tag, push — GitHub Actions builds automatically)
---

# Release Workflow

// turbo-all

## Pre-release Checklist

1. Make sure all changes are committed and `git status` is clean
2. Run tests: `npm test` — all must pass

## Version Bump

3. Update version in `src-tauri/tauri.conf.json` → `"version": "x.x.x"`
4. Update version in `package.json` → `"version": "x.x.x"`
5. Update `docs/api/version/index.json`:
   - `version` → new version
   - `releaseDate` → today (YYYY-MM-DD)
   - `releaseNotes` → brief summary of changes

## Commit & Tag

5. Commit version bump:
```powershell
git add src-tauri/tauri.conf.json package.json
git commit -m "release: vX.X.X"
```

6. Create and push tag (this triggers GitHub Actions automatically):
```powershell
git tag vX.X.X
git push origin main --tags
```

## Post-release

7. Monitor GitHub Actions: https://github.com/edison7009/Echobird/actions
   - Builds: Windows (.msi/.exe), macOS arm64, macOS x86_64, Linux (.AppImage/.deb)
   - Takes ~15–25 minutes
8. Go to GitHub Releases page → the draft release will appear automatically
9. Edit the release notes, then click **Publish release**
10. Clean up: remove any `.yml` and `.blockmap` files from the release assets if present

