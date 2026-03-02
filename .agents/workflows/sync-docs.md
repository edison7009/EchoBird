---
description: Sync website docs from private Echobird repo to public Echobird-MotherAgent repo
---

# Docs Sync Workflow

When you update the website (`docs/`) in the private repo, you need to manually sync it to the public repo so Cloudflare Pages picks up the changes.

---

## Step 1: Edit docs in private repo

Make your changes inside `d:\Echobird\docs\`.

---

## Step 2: Commit to private repo

```powershell
git -C "d:\Echobird" add docs/
git -C "d:\Echobird" commit -m "docs: <describe your change>"
git -C "d:\Echobird" push origin main
```

---

## Step 3: Sync to public repo

// turbo
```powershell
Copy-Item -Path "d:\Echobird\docs\*" -Destination "d:\Echobird-MotherAgent\" -Recurse -Force
git -C "d:\Echobird-MotherAgent" add -A
git -C "d:\Echobird-MotherAgent" commit -m "docs: sync from private repo"
git -C "d:\Echobird-MotherAgent" push origin main
```

Cloudflare Pages will automatically detect the push and redeploy the website (usually within 1-2 minutes).

---

## Note on link paths

- In `d:\Echobird\docs\README.*.md` language files: use `../README.md` for English link
- In `d:\Echobird-MotherAgent\README.*.md` language files: use `./README.md` for English link
- The sync command above handles copying; just remember to update both if editing README language files directly
