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
Copy-Item -Path "d:\Echobird\docs\*" -Destination "d:\Echobird-MotherAgent\docs\" -Recurse -Force
git -C "d:\Echobird-MotherAgent" add -A
git -C "d:\Echobird-MotherAgent" commit -m "docs: sync from private repo"
git -C "d:\Echobird-MotherAgent" push origin main
```

Cloudflare Pages will automatically detect the push and redeploy the website (usually within 1-2 minutes).

---

## Note on link paths

Cloudflare Pages uses `docs/` as the website root directory. URL mapping:

| File in repo | Website URL |
|---|---|
| `docs/index.html` | `https://echobird.ai/` |
| `docs/roles/1.jpg` | `https://echobird.ai/roles/1.jpg` |
| `docs/api/version/index.json` | `https://echobird.ai/api/version/index.json` |

> [!IMPORTANT]
> The `docs/` prefix is stripped in the URL. Use `echobird.ai/roles/` not `echobird.ai/docs/roles/`.

Both repos have the same `docs/` directory structure. All paths are identical between private and public repos:

- Language READMEs in `docs/` use `./icon.png`, `./1.png` etc. (relative to `docs/`)
- Language READMEs use `../README.md` to link back to the English root README
- The root `README.md` uses `docs/icon.png`, `docs/1.png` etc.
- The sync command copies `docs/*` to `docs/` in both repos, so paths are always consistent.


