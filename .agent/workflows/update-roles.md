---
description: Update role JSON files from upstream agency-agents repos (EN + ZH)
---

# Update Roles Workflow

Updates role data from two upstream GitHub repos. This involves **4 deliverables**:

1. `docs/roles/roles-en.json` and `docs/roles/roles-zh-Hans.json` 鈥?role catalog loaded by app from CDN
2. `docs/roles/en/` and `docs/roles/zh-Hans/` 鈥?actual role MD files served by Cloudflare Pages
3. `docs/roles/*.png` 鈥?role avatar images (one per role, sequential numbering)
4. Sync `docs/` to public repo for Cloudflare deployment

> [!IMPORTANT]
> The app loads role JSON from CDN (`echobird.ai/roles/roles-{lang}.json`), NOT from local files.
> No app release is needed for role updates 鈥?just push docs to public repo.

> [!CAUTION]
> **Upstream repos have NESTED subdirectories.** For example `game-development/blender/`, `game-development/unity/`, `integrations/mcp-memory/`.
> ALL scanning and copying MUST be **recursive** (`os.walk` / `-Recurse`). Single-level scanning will miss roles!

## Source Repos

| Language | Repo | Bridge download URL pattern |
|---|---|---|
| English | `msitarzewski/agency-agents` | `https://raw.githubusercontent.com/msitarzewski/agency-agents/main/{filePath}` |
| Chinese | `jnMetaCode/agency-agents-zh` | `https://raw.githubusercontent.com/jnMetaCode/agency-agents-zh/main/{filePath}` |

## Step 1: Clone/update upstream repos

// turbo
```powershell
if (Test-Path "D:\tmp\agency-agents") { git -C "D:\tmp\agency-agents" pull } else { git clone https://github.com/msitarzewski/agency-agents.git "D:\tmp\agency-agents" }
if (Test-Path "D:\tmp\agency-agents-zh") { git -C "D:\tmp\agency-agents-zh" pull } else { git clone https://github.com/jnMetaCode/agency-agents-zh.git "D:\tmp\agency-agents-zh" }
```

## Step 2: Regenerate JSON files

Use `D:\Echobird\scripts\gen_roles.py` script. It:
1. **Recursively** scans category directories with `os.walk` (handles nested dirs like `game-development/blender/`)
2. Uses a **fixed CAT_ORDER** to ensure consistent role sorting.
3. Parses YAML frontmatter for `name` and `description`.
4. Assigns images based on **filePath** (`docs/roles/{locale}/{category}/{role}.png`).
   - Example: `https://echobird.ai/roles/en/design/design-brand-guardian.png`
5. `filePath` includes full relative path (e.g. `game-development/blender/blender-addon-engineer.md`).
6. Writes to `docs/roles/roles-en.json` and `docs/roles/roles-zh-Hans.json`.

// turbo
```powershell
python D:\Echobird\scripts\gen_roles.py
```

> [!IMPORTANT]
> Imaging naming is now **self-documenting**. Every `.md` file in `docs/roles/` MUST have a corresponding `.png` file in the same directory with the same basename.

## Step 3: Sync MD and PNG files to docs/

Copy upstream MDs and their corresponding images **recursively** into `docs/roles/en/` and `docs/roles/zh-Hans/`.

> [!WARNING]
> You MUST use `-Recurse` to copy ALL nested subdirectories. Shared roles (same ID in EN and ZH) SHOULD use the same image file.

```powershell
$skipDirs = @('scripts','strategy','examples','.github','.git')
$skipFiles = @('README.md','README.zh-TW.md','EXECUTIVE-BRIEF.md','QUICKSTART.md','CONTRIBUTING.md','nexus-strategy.md')

# EN 鈥?recursive copy (MD + PNG)
$enSrc = "D:\tmp\agency-agents"; $enDst = "D:\Echobird\docs\roles\en"
Get-ChildItem $enSrc -Directory | Where-Object { $_.Name -notin $skipDirs } | ForEach-Object {
    Get-ChildItem $_.FullName -Recurse -Include *.md, *.png | Where-Object { $_.Name -notin $skipFiles } | ForEach-Object {
        $relDir = $_.Directory.FullName.Replace($enSrc + "\", "")
        $dstDir = Join-Path $enDst $relDir
        if (!(Test-Path $dstDir)) { New-Item -ItemType Directory -Path $dstDir -Force | Out-Null }
        Copy-Item $_.FullName (Join-Path $dstDir $_.Name) -Force
    }
}

# ZH 鈥?recursive copy (MD + PNG)
$zhSrc = "D:\tmp\agency-agents-zh"; $zhDst = "D:\Echobird\docs\roles\zh-Hans"
Get-ChildItem $zhSrc -Directory | Where-Object { $_.Name -notin $skipDirs } | ForEach-Object {
    Get-ChildItem $_.FullName -Recurse -Include *.md, *.png | Where-Object { $_.Name -notin $skipFiles } | ForEach-Object {
        $relDir = $_.Directory.FullName.Replace($zhSrc + "\", "")
        $dstDir = Join-Path $zhDst $relDir
        if (!(Test-Path $dstDir)) { New-Item -ItemType Directory -Path $dstDir -Force | Out-Null }
        Copy-Item $_.FullName (Join-Path $dstDir $_.Name) -Force
    }
}
```

## Step 4: Maintenance of Images

If a new role is added upstream WITHOUT an image, or if you want to change an image:
1. Add/Update `{role-name}.png` in the source directory next to the `.md` file.
2. Run this workflow to sync and regenerate JSON.
3. For shared roles, ensure the `.png` is identical in both `en` and `zh-Hans` folders to maintain consistency.

## Step 5: Compare and commit

// turbo
```powershell
git -C "D:\Echobird" diff --stat docs/roles/
```

```powershell
git -C "D:\Echobird" add docs/roles/
git -C "D:\Echobird" commit -m "chore: update roles from upstream agency-agents repos"
git -C "D:\Echobird" push origin main
```

## Step 6: Sync docs to public repo

```powershell
Copy-Item -Path "D:\Echobird\docs\roles\*" -Destination "D:\Echobird-MotherAgent\docs\roles\" -Recurse -Force
git -C "D:\Echobird-MotherAgent" add -A
git -C "D:\Echobird-MotherAgent" commit -m "docs: sync roles from private repo"
git -C "D:\Echobird-MotherAgent" -c core.editor=true pull --rebase -X theirs origin main
git -C "D:\Echobird-MotherAgent" push origin main
```

> [!NOTE]
> This push only triggers **Cloudflare Pages** redeploy (1-2 min). It does **NOT** trigger CI build 鈥?CI only runs on `repository_dispatch` events (triggered by tag push from the private repo's `release.yml`).
> No app release needed 鈥?users see updated roles immediately via CDN.

