---
description: Update role JSON files from upstream agency-agents repos (EN + ZH)
---

# Update Roles Workflow

Regenerates `roles/roles-en.json` and `roles/roles-zh-Hans.json` from two upstream GitHub repos.

> [!IMPORTANT]
> The `roles/` directory only contains 2 JSON files. No `.md` files or reference repos are stored locally.
> Bridge CLI downloads `.md` files at runtime via raw GitHub URLs — no local copy needed.

## Source Repos

| Language | Repo | Raw URL base |
|---|---|---|
| English | `msitarzewski/agency-agents` | `https://raw.githubusercontent.com/msitarzewski/agency-agents/main/en/` |
| 中文 | `jnMetaCode/agency-agents-zh` | `https://raw.githubusercontent.com/jnMetaCode/agency-agents-zh/main/zh-Hans/` |

## Step 1: Clone/update upstream repos to temp directories

// turbo
```powershell
# English
if (Test-Path "D:\tmp\agency-agents") {
    git -C "D:\tmp\agency-agents" pull
} else {
    git clone https://github.com/msitarzewski/agency-agents.git "D:\tmp\agency-agents"
}

# Chinese
if (Test-Path "D:\tmp\agency-agents-zh") {
    git -C "D:\tmp\agency-agents-zh" pull
} else {
    git clone https://github.com/jnMetaCode/agency-agents-zh.git "D:\tmp\agency-agents-zh"
}

# Show counts
$enCount = (Get-ChildItem "D:\tmp\agency-agents\en" -Recurse -Filter *.md).Count
$zhCount = (Get-ChildItem "D:\tmp\agency-agents-zh\zh-Hans" -Recurse -Filter *.md).Count
Write-Host "EN: $enCount roles, ZH: $zhCount roles"
```

## Step 2: Regenerate JSON files

Scan the temp directories and rebuild the two JSON files:

1. Scan each `D:\tmp\agency-agents\en\{category}\` and `D:\tmp\agency-agents-zh\zh-Hans\{category}\` directory
2. Parse YAML frontmatter from each `.md` file to extract `name`, `description`
3. Build the JSON structure with `categories`, `roles` (id, name, description, category, filePath, img)
4. Image assignment: cycle through the 17 placeholder images using CDN URL `https://echobird.ai/docs/roles/{n}.jpg`
5. Write to `roles/roles-en.json` and `roles/roles-zh-Hans.json`

> [!IMPORTANT]
> The `img` field uses CDN URLs like `https://echobird.ai/docs/roles/4.jpg`.
> The `filePath` field is relative, e.g. `engineering/engineering-ai-engineer.md`.
> Bridge CLI constructs the full raw GitHub URL at runtime:
> - EN: `https://raw.githubusercontent.com/msitarzewski/agency-agents/main/en/{filePath}`
> - ZH: `https://raw.githubusercontent.com/jnMetaCode/agency-agents-zh/main/zh-Hans/{filePath}`

## Step 3: Compare and verify

// turbo
```powershell
git -C "D:\Echobird" diff --stat roles/roles-en.json roles/roles-zh-Hans.json
```

## Step 4: Commit

```powershell
git -C "D:\Echobird" add roles/roles-en.json roles/roles-zh-Hans.json
git -C "D:\Echobird" commit -m "chore: update roles from upstream agency-agents repos"
git -C "D:\Echobird" push origin main
```
