---
description: Release a new version of Echobird (bump version, sync files, tag, push)
---

# Release Workflow

// turbo-all

## Pre-release Checklist

1. **Read BUILD.md first** — `internal-docs/BUILD.md` contains version history and release notes format
2. **Run tests locally**: `npm test` — all tests must pass before release

## Version Bump

3. Update version in `package.json` → `"version": "x.x.x"`
4. Sync `docs/api/version/index.json`:
   - `version` → new version
   - `releaseDate` → today (YYYY-MM-DD)
   - `releaseNotes` → brief summary of changes
5. Update `internal-docs/BUILD.md`:
   - Version history table (line ~238): add new version row
   - "当前版本" text (line ~234): update to new version

## Commit & Tag

6. Commit all changes:
```powershell
git add -A
git commit -m "release: vX.X.X - brief description"
```

7. Create and push tag:
```powershell
git tag vX.X.X
git push origin main --tags
```

## Post-release

8. Monitor GitHub Actions: https://github.com/edison7009/Echobird/actions
9. Verify all 3 platforms pass (Windows/macOS/Linux)
10. Check GitHub Releases page for uploaded assets (.exe, .dmg, .AppImage)
11. Clean up Release assets: remove `.yml` and `.blockmap` files if present
