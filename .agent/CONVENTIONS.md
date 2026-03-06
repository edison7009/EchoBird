# Development Conventions

## File Encoding (CRITICAL)

**NEVER** use PowerShell's `Get-Content`/`Set-Content` to modify ANY source files — they change the encoding (UTF-16 LE or system locale), which **breaks builds** (Vite, Cargo, etc.).

This applies to ALL files: `.ts`, `.tsx`, `.rs`, `.json`, `.md` — not just CJK files.

### v2.2.5 incident

`Set-Content` on 28 i18n `.ts` files changed encoding from UTF-8 to UTF-16 LE → Vite reported `unterminated string literal` → all 4 CI platform builds failed.

### Safe approaches for text replacement:

1. **Preferred**: Use the `write_to_file` or `replace_file_content` tools directly
2. **If must use PowerShell**: Use .NET API with explicit UTF-8 no-BOM:
   ```powershell
   $utf8 = [System.Text.UTF8Encoding]::new($false)
   $content = [System.IO.File]::ReadAllText($path, $utf8)
   $content = $content.Replace('old', 'new')
   [System.IO.File]::WriteAllText($path, $content, $utf8)
   ```
3. **NEVER**: `(Get-Content $f) -replace ... | Set-Content $f`  ← DESTROYS ENCODING
4. **NEVER**: `$content | Set-Content $f -NoNewline`  ← ALSO DESTROYS ENCODING
