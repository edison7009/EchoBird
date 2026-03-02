# Development Conventions

## File Encoding (CRITICAL)

This project contains Chinese (CJK) text in `internal-docs/` markdown files.

**NEVER** use PowerShell's `Get-Content`/`Set-Content` to modify files with CJK characters — they default to the system locale encoding (not UTF-8), which **corrupts** Chinese text.

### Safe approaches for text replacement in CJK files:

1. **Preferred**: Use the `write_to_file` or `replace_file_content` tools directly
2. **If must use PowerShell**: Use .NET API with explicit UTF-8:
   ```powershell
   $content = [System.IO.File]::ReadAllText($path, [System.Text.Encoding]::UTF8)
   $content = $content -replace 'old', 'new'
   [System.IO.File]::WriteAllText($path, $content, [System.Text.UTF8Encoding]::new($false))
   ```
3. **NEVER**: `(Get-Content $f) -replace ... | Set-Content $f`  ← THIS DESTROYS CJK TEXT
