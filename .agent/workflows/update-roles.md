---
description: Update role JSON files from upstream agency-agents repos (EN + ZH)
---

# Update Roles Workflow

Regenerates `roles/roles-en.json` and `roles/roles-zh-Hans.json` from the two upstream GitHub repos.

## Source Repos

| Language | Repo | Raw URL base |
|---|---|---|
| English | `msitarzewski/agency-agents` | `https://raw.githubusercontent.com/msitarzewski/agency-agents/main/en/` |
| 中文 | `jnMetaCode/agency-agents-zh` | `https://raw.githubusercontent.com/jnMetaCode/agency-agents-zh/main/zh-Hans/` |

## Step 1: Clone/update the upstream repos to temp directories

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
```

## Step 2: Copy .md files to local roles/ directory

// turbo
```powershell
# English: copy all category dirs with .md files
Copy-Item -Path "D:\tmp\agency-agents\en\*" -Destination "D:\Echobird\roles\en\" -Recurse -Force

# Chinese: copy all category dirs with .md files
Copy-Item -Path "D:\tmp\agency-agents-zh\zh-Hans\*" -Destination "D:\Echobird\roles\zh-Hans\" -Recurse -Force

# Show counts
$enCount = (Get-ChildItem "D:\Echobird\roles\en" -Recurse -Filter *.md).Count
$zhCount = (Get-ChildItem "D:\Echobird\roles\zh-Hans" -Recurse -Filter *.md).Count
Write-Host "EN: $enCount roles, ZH: $zhCount roles"
```

## Step 3: Regenerate JSON files

Ask the AI to regenerate the two JSON files by scanning the .md directories:

1. Scan each `roles/{locale}/{category}/` directory
2. Parse YAML frontmatter from each `.md` file to extract `name`, `description`
3. Build the JSON structure with `categories`, `roles` (id, name, description, category, filePath, img)
4. Image assignment: cycle through the 17 placeholder images (`1.jpg` to `17.jpg/png`) for the `img` field using CDN URL `https://echobird.ai/docs/roles/{n}.jpg`
5. Write to `roles/roles-en.json` and `roles/roles-zh-Hans.json`

> [!IMPORTANT]
> The `img` field in the JSON uses CDN URLs like `https://echobird.ai/docs/roles/4.jpg`.
> The `filePath` field is relative, e.g. `engineering/engineering-ai-engineer.md`.
> Bridge CLI will construct the full raw GitHub URL at runtime:
> - EN: `https://raw.githubusercontent.com/msitarzewski/agency-agents/main/en/{filePath}`
> - ZH: `https://raw.githubusercontent.com/jnMetaCode/agency-agents-zh/main/zh-Hans/{filePath}`

## Step 4: Compare and verify

// turbo
```powershell
# Check for new/removed roles
git -C "D:\Echobird" diff --stat roles/roles-en.json roles/roles-zh-Hans.json
```

## Step 5: Commit

```powershell
git -C "D:\Echobird" add roles/
git -C "D:\Echobird" commit -m "chore: update roles from upstream agency-agents repos"
git -C "D:\Echobird" push origin main
```
