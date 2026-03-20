---
description: Update role JSON files from upstream agency-agents repos (EN + ZH)
---

# Update Roles Workflow

Updates role data from two upstream GitHub repos. This involves **4 deliverables**:

1. `roles/roles-en.json` and `roles/roles-zh-Hans.json` — role catalog for the app
2. `docs/roles/en/` and `docs/roles/zh-Hans/` — actual role MD files served by Cloudflare Pages
3. `docs/roles/*.png` — role avatar images (one per role, sequential numbering)
4. Sync `docs/` to public repo for Cloudflare deployment

> [!IMPORTANT]
> The upstream repos have MDs directly under category directories (e.g. `engineering/xxx.md`), NOT under `en/` or `zh-Hans/` subdirectories.

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
1. Scans category directories (skips `scripts/`, `strategy/`, `examples/`, `.github/`)
2. Parses YAML frontmatter for `name` and `description`
3. Assigns sequential image numbers (`https://echobird.ai/roles/{n}.png`) — one unique image per role, NO cycling
4. Writes `roles/roles-en.json` and `roles/roles-zh-Hans.json`

// turbo
```powershell
python C:\tmp\gen_roles.py
```

> [!CAUTION]
> Image numbering is **sequential per role** (1, 2, 3, ..., N). If new roles are added and the number exceeds existing images in `docs/roles/`, you must generate new images (see Step 4).

## Step 3: Sync MD files to docs/

Copy upstream MDs into `docs/roles/en/` and `docs/roles/zh-Hans/` for Cloudflare Pages serving.

```powershell
$skipDirs = @('scripts','strategy','examples','.github','.git')
$skipFiles = @('README.md','README.zh-TW.md','EXECUTIVE-BRIEF.md','QUICKSTART.md','CONTRIBUTING.md','nexus-strategy.md')

# EN
$enSrc = "D:\tmp\agency-agents"; $enDst = "D:\Echobird\docs\roles\en"
Get-ChildItem $enSrc -Directory | Where-Object { $_.Name -notin $skipDirs } | ForEach-Object {
    $dst = Join-Path $enDst $_.Name; if (!(Test-Path $dst)) { New-Item -ItemType Directory -Path $dst -Force | Out-Null }
    Get-ChildItem $_.FullName -Filter *.md | Where-Object { $_.Name -notin $skipFiles } | ForEach-Object { Copy-Item $_.FullName (Join-Path $dst $_.Name) -Force }
}

# ZH
$zhSrc = "D:\tmp\agency-agents-zh"; $zhDst = "D:\Echobird\docs\roles\zh-Hans"
Get-ChildItem $zhSrc -Directory | Where-Object { $_.Name -notin $skipDirs } | ForEach-Object {
    $dst = Join-Path $zhDst $_.Name; if (!(Test-Path $dst)) { New-Item -ItemType Directory -Path $dst -Force | Out-Null }
    Get-ChildItem $_.FullName -Filter *.md | Where-Object { $_.Name -notin $skipFiles } | ForEach-Object { Copy-Item $_.FullName (Join-Path $dst $_.Name) -Force }
}
```

## Step 4: Generate missing role images

Check if new roles exceed existing image count. If so, generate new avatar images.

```powershell
$maxImg = (Get-ChildItem "D:\Echobird\docs\roles\*.png" | ForEach-Object { [int]($_.BaseName) } | Measure-Object -Maximum).Maximum
$maxRole = [math]::Max(141, 165)  # update with actual counts from Step 2
Write-Host "Max image: $maxImg, Max roles: $maxRole, Need new: $($maxRole -gt $maxImg)"
```

If new images needed: use `generate_image` tool to create role avatars, save as `docs/roles/{N}.png`.

## Step 5: Compare and commit

// turbo
```powershell
git -C "D:\Echobird" diff --stat roles/ docs/roles/
```

```powershell
git -C "D:\Echobird" add roles/ docs/roles/
git -C "D:\Echobird" commit -m "chore: update roles from upstream agency-agents repos"
git -C "D:\Echobird" push origin main
```

## Step 6: Sync docs to public repo

```powershell
Copy-Item -Path "D:\Echobird\docs\*" -Destination "D:\Echobird-MotherAgent\docs\" -Recurse -Force
git -C "D:\Echobird-MotherAgent" add -A
git -C "D:\Echobird-MotherAgent" commit -m "docs: sync roles from private repo"
git -C "D:\Echobird-MotherAgent" -c core.editor=true pull --rebase origin main
git -C "D:\Echobird-MotherAgent" push origin main
```
