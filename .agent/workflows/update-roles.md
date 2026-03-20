---
description: Update role JSON files from upstream agency-agents repos (EN + ZH)
---

# Update Roles Workflow

Updates role data from two upstream GitHub repos. This involves **4 deliverables**:

1. `docs/roles/roles-en.json` and `docs/roles/roles-zh-Hans.json` — role catalog loaded by app from CDN
2. `docs/roles/en/` and `docs/roles/zh-Hans/` — actual role MD files served by Cloudflare Pages
3. `docs/roles/*.png` — role avatar images (one per role, sequential numbering)
4. Sync `docs/` to public repo for Cloudflare deployment

> [!IMPORTANT]
> The app loads role JSON from CDN (`echobird.ai/roles/roles-{lang}.json`), NOT from local files.
> No app release is needed for role updates — just push docs to public repo.

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

Use `C:\tmp\gen_roles.py` script. It:
1. **Recursively** scans category directories with `os.walk` (handles nested dirs like `game-development/blender/`)
2. Skips `scripts/`, `strategy/`, `examples/`, `.github/`
3. Parses YAML frontmatter for `name` and `description`
4. Assigns sequential image numbers (`https://echobird.ai/roles/{n}.png`) — one unique image per role, NO cycling
5. `filePath` includes full relative path (e.g. `game-development/blender/blender-addon-engineer.md`)
6. Writes to `docs/roles/roles-en.json` and `docs/roles/roles-zh-Hans.json`

// turbo
```powershell
python C:\tmp\gen_roles.py
```

> [!CAUTION]
> Image numbering is **sequential per role** (1, 2, 3, ..., N). If new roles are added and the number exceeds existing images in `docs/roles/`, you must generate new images (see Step 4).

## Step 3: Sync MD files to docs/

Copy upstream MDs **recursively** into `docs/roles/en/` and `docs/roles/zh-Hans/`.

> [!WARNING]
> You MUST use `-Recurse` to copy ALL nested subdirectories (e.g. `game-development/blender/`, `integrations/mcp-memory/`). Without `-Recurse`, nested role files will be silently skipped!

```powershell
$skipDirs = @('scripts','strategy','examples','.github','.git')
$skipFiles = @('README.md','README.zh-TW.md','EXECUTIVE-BRIEF.md','QUICKSTART.md','CONTRIBUTING.md','nexus-strategy.md')

# EN — recursive copy
$enSrc = "D:\tmp\agency-agents"; $enDst = "D:\Echobird\docs\roles\en"
Get-ChildItem $enSrc -Directory | Where-Object { $_.Name -notin $skipDirs } | ForEach-Object {
    Get-ChildItem $_.FullName -Recurse -Filter *.md | Where-Object { $_.Name -notin $skipFiles } | ForEach-Object {
        $relDir = $_.Directory.FullName.Replace($enSrc + "\", "")
        $dstDir = Join-Path $enDst $relDir
        if (!(Test-Path $dstDir)) { New-Item -ItemType Directory -Path $dstDir -Force | Out-Null }
        Copy-Item $_.FullName (Join-Path $dstDir $_.Name) -Force
    }
}

# ZH — recursive copy
$zhSrc = "D:\tmp\agency-agents-zh"; $zhDst = "D:\Echobird\docs\roles\zh-Hans"
Get-ChildItem $zhSrc -Directory | Where-Object { $_.Name -notin $skipDirs } | ForEach-Object {
    Get-ChildItem $_.FullName -Recurse -Filter *.md | Where-Object { $_.Name -notin $skipFiles } | ForEach-Object {
        $relDir = $_.Directory.FullName.Replace($zhSrc + "\", "")
        $dstDir = Join-Path $zhDst $relDir
        if (!(Test-Path $dstDir)) { New-Item -ItemType Directory -Path $dstDir -Force | Out-Null }
        Copy-Item $_.FullName (Join-Path $dstDir $_.Name) -Force
    }
}
```

## Step 4: Generate missing role images

Check if new roles exceed existing image count. If so, generate new avatar images or copy from existing ones.

```powershell
$maxImg = (Get-ChildItem "D:\Echobird\docs\roles\*.png" | ForEach-Object { [int]($_.BaseName) } | Measure-Object -Maximum).Maximum
# Use actual counts from Step 2 output
$maxRole = [math]::Max($enCount, $zhCount)
Write-Host "Max image: $maxImg, Max roles: $maxRole, Need new: $($maxRole -gt $maxImg)"
```

If new images needed: use `generate_image` tool to create role avatars, or copy existing ones as placeholders, save as `docs/roles/{N}.png`.

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
> This push only triggers **Cloudflare Pages** redeploy (1-2 min). It does **NOT** trigger CI build — CI only runs on `repository_dispatch` events (triggered by tag push from the private repo's `release.yml`).
> No app release needed — users see updated roles immediately via CDN.
