---
description: Encoding and file format rules for all code modifications
---

# Encoding Standards

All source files in this project MUST use **UTF-8 (no BOM)** encoding with **CRLF** line endings.

## Rules

1. **Encoding**: UTF-8 without BOM. Never use GBK, GB2312, Latin-1, or any other encoding.
2. **Line endings**: CRLF (`\r\n`) on all text files.
3. **Final newline**: All files must end with a single newline.
4. **No BOM**: Never add a UTF-8 BOM (0xEF 0xBB 0xBF) to any file.

## When Editing Files

> [!CAUTION]
> **NEVER use `Set-Content -Encoding UTF8`** — PowerShell adds BOM (0xEF 0xBB 0xBF) which breaks
> `tauri-action` (JSON parse error) and `cargo` (TOML parse error: Unknown character "65279").

### ❌ WRONG — adds BOM:
```powershell
Set-Content "file.json" $content -Encoding UTF8
(Get-Content "file.json" -Raw) -replace 'a','b' | Set-Content "file.json" -Encoding UTF8
```

### ✅ CORRECT — no BOM:
```powershell
[System.IO.File]::WriteAllText("full\path\file.json", $content, [System.Text.UTF8Encoding]::new($false))
```

### Emergency fix (BOM already committed):
```powershell
foreach ($f in @('package.json','src-tauri\tauri.conf.json','src-tauri\Cargo.toml')) {
    $b = [System.IO.File]::ReadAllBytes($f)
    if ($b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF) {
        [System.IO.File]::WriteAllBytes($f, $b[3..$b.Length])
        Write-Host "BOM removed: $f"
    }
}
git add -A; git commit -m "fix: remove BOM"; git push
```

- If a file read returns garbled characters (mojibake), **STOP** — do not write the garbled content back.
- If the file is truly non-UTF-8, convert it first:
  ```powershell
  $bytes = [System.IO.File]::ReadAllBytes("path")
  $text = [System.Text.Encoding]::Default.GetString($bytes)
  [System.IO.File]::WriteAllText("path", $text, [System.Text.UTF8Encoding]::new($false))
  ```


## Language Rules

- All comments in source code, CSS, and config files MUST be in **English**.
- i18n translation data files (`src/i18n/*.ts`) contain translated strings as data — those are fine.
- Section header comments in i18n files (`// Navigation`, `// Buttons`, etc.) MUST be in English.

## Reference Files

- `.editorconfig` — IDE-level encoding enforcement
- `.gitattributes` — Git-level line ending normalization
