---
description: How to safely add a new i18n key to all 28 language files
---

# Adding a new i18n key to all 28 languages

> **Golden rule**: Never use PowerShell string operations to write i18n files directly.
> Always use the `write_to_file` tool or the validation script below.

## Step 1: Add the key to `en.ts` first (source of truth)

Add the new key to `src/i18n/en.ts`. This is the reference file.

```typescript
'your.new.key': 'English value',
```

Make sure the line above has a trailing comma. Run quick check:

// turbo
```powershell
npx vite build 2>&1 | Select-String "error|built in" | Select-Object -First 5
```

## Step 2: Add to all other language files using `write_to_file` tool only

**NEVER** use PowerShell `Replace` or string concatenation to add i18n keys.
Always use the `write_to_file` (Overwrite) tool or `multi_replace_file_content` tool.

For each language file, use `multi_replace_file_content` to append the key **before the closing `};`**:

TargetContent: `    'common.showProcess':` ... `,\n};`
ReplacementContent: add the new key before `};`

Order of files to update (28 total):
`ar`, `bn`, `cs`, `de`, `el`, `en`, `es`, `fa`, `fi`, `fr`, `he`, `hi`, `hu`, `id`, `it`, `ja`, `ko`, `ms`, `nl`, `pl`, `pt`, `ru`, `sv`, `th`, `tr`, `vi`, `zh-Hans`, `zh-Hant`

## Step 3: Validate ALL files pass — run this after every batch

// turbo
```powershell
$utf8 = [System.Text.UTF8Encoding]::new($false)
$repl = [char]0xFFFD
$errors = 0
Get-ChildItem "src\i18n\*.ts" | Where-Object { $_.Name -ne 'types.ts' -and $_.Name -ne 'index.ts' } | Sort-Object Name | ForEach-Object {
    $text = [System.IO.File]::ReadAllText($_.FullName, $utf8)
    $fffd  = ($text.ToCharArray() | Where-Object { $_ -eq $repl }).Count
    $lines = $text -split "\r?\n"
    $badQ  = 0
    foreach ($line in $lines) {
        if ($line -match "^\s+'[^']+':") {
            $s = $line -replace "\\'"
            $q = ($s.ToCharArray() | Where-Object { $_ -eq "'" }).Count
            if ($q % 2 -ne 0) { $badQ++ }
        }
    }
    if ($fffd -gt 0 -or $badQ -gt 0) {
        Write-Host "FAIL $($_.Name): fffd=$fffd badQuotes=$badQ"
        $errors++
    } else {
        Write-Host "OK   $($_.Name)"
    }
}
if ($errors -eq 0) { Write-Host "`nAll $((Get-ChildItem 'src\i18n\*.ts' | Where-Object { $_.Name -ne 'types.ts' -and $_.Name -ne 'index.ts' }).Count) language files clean!" }
```

## Step 4: Build verification

// turbo
```powershell
npx vite build 2>&1 | Select-String "error during build|ERROR|built in" | Select-Object -First 5
```

If build fails with `Unterminated string literal`:
1. Note the file and line number from the error
2. Open the file and find the line
3. Check if the string has an even number of single quotes
4. Common fixes:
   - Missing trailing comma on previous line → add `,`
   - U+FFFD character in string → replace with correct char (`→` `—` `…` etc.)
   - String not closed → add closing `'`

## Step 5: Commit

```powershell
git add src/i18n/
git commit -m "i18n: add <key-name> to all 28 languages"
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
