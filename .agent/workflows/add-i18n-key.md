---
description: How to safely add a new i18n key to en.ts and zh-Hans.ts
---

# Adding a new i18n key

> **Project supports only 2 languages: `en` (English) and `zh-Hans` (Simplified Chinese).**
> All other language files have been removed. Do NOT add them back without discussion.

> **Golden rule**: NEVER use PowerShell `Set-Content`, `>` redirect, or string replace to write i18n files.
> Always use `multi_replace_file_content` tool or `[System.IO.File]::WriteAllText` with explicit UTF-8 no-BOM.

## Step 1: Add the key to `en.ts` (source of truth)

Add the new key to `src/i18n/en.ts`:

```typescript
'your.new.key': 'English value',
```

Also add to `src/i18n/types.ts` in the `TKey` union type.

## Step 2: Add to `zh-Hans.ts` using `multi_replace_file_content` tool only

Use `multi_replace_file_content` to insert the translated value.
**NEVER** use PowerShell string operations — they corrupt non-ASCII characters.

If batch-patching in PowerShell is needed, use ONLY this safe pattern:
```powershell
$path = (Resolve-Path "src\i18n\zh-Hans.ts").Path
$bytes = [System.IO.File]::ReadAllBytes($path)
$content = [System.Text.Encoding]::UTF8.GetString($bytes)
# ... string manipulation ...
[System.IO.File]::WriteAllText($path, $newContent, [System.Text.UTF8Encoding]::new($false))
```

## Step 3: Validate both files

// turbo
```powershell
$utf8 = [System.Text.UTF8Encoding]::new($false)
$repl = [char]0xFFFD
$errors = 0
Get-ChildItem "src\i18n\*.ts" | Where-Object { $_.Name -notin @('types.ts','index.ts') } | ForEach-Object {
    $text = [System.IO.File]::ReadAllText($_.FullName, $utf8)
    $fffd = ($text.ToCharArray() | Where-Object { $_ -eq $repl }).Count
    if ($fffd -gt 0) { Write-Host "FAIL $($_.Name): fffd=$fffd"; $errors++ }
    else { Write-Host "OK   $($_.Name)" }
}
if ($errors -eq 0) { Write-Host "All language files clean!" }
```

## Step 4: Build verification

// turbo
```powershell
npx vite build 2>&1 | Select-String "error during build|ERROR|built in" | Select-Object -First 5
```

## Step 5: Commit

```powershell
git add src/i18n/
git commit -m "i18n: add <key-name> to en + zh-Hans"
git push origin main
```

---

## Special Characters Reference

When writing translations, use these characters directly (NOT escape sequences):
| Symbol | Unicode | Correct usage |
|--------|---------|--------------|
| → | U+2192 | Navigation steps: `A → B → C` |
| — | U+2014 | Em dash: `failed — retry` |
| … | U+2026 | Ellipsis (prefer `...` for animated-dot strings) |
| ⚡ | U+26A1 | GPU indicator |
| ✔ | U+2714 | Ready indicator |
| ✓ | U+2713 | Checkmark |

**These characters WILL be corrupted if:**
- Written via `Set-Content` without explicit `-Encoding UTF8`
- Written via PowerShell `>` redirect
- Copied from a GBK/GB2312 terminal output

**Safe write method in PowerShell:**
```powershell
[System.IO.File]::WriteAllText($path, $content, [System.Text.UTF8Encoding]::new($false))
```
