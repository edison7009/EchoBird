# AGENTS.md — EchoBird

Instructions for agents (ZCode / Claude Code / Codex) working in this repo.
Read this before modifying code.

## Project shape

EchoBird targets Windows, macOS, and Linux desktop. React + TypeScript + Vite
frontend lives in `src/`, and the Rust backend lives in `src-tauri/`. The desktop
binary links `echobird_lib` as an `rlib`; verify native desktop build and packaging
when changing library output types.

Two-tier tool/feature registries often ship in **both** a bundled copy and a remote copy (e.g. `src/data/<x>.json` vs
`docs/api/<...>/index.json`) — when adding or editing an entry, update **both**
files unless told otherwise; they share one schema and the remote wins at
runtime. Prettier governs the frontend JSON/TS; rustfmt governs `src-tauri/`.

Keep internal development reports, research notes, and integration playbooks
in the ignored `internal-docs/` directory. `docs/` is the public website.

## Main branch workflow

- Work directly on the checkout of `main` by default. Use one development
  workflow per project. Create a development branch, separate worktree, or
  parallel chat only when the user explicitly requests it.
- Before editing, check the current branch and existing changes. Preserve any
  work already in progress. If access or existing changes block work on
  `main`, report the specific blocker.
- Commit authorized changes directly to `main`. When pushing or releasing,
  confirm the intended commits are on `origin/main`. Integrate any intended
  changes still on other branches or worktrees before delivery.
- Stage explicit task-owned paths and inspect the staged diff before committing.
  Each commit must contain the changes for its stated task.

## Desktop interaction rules

- EchoBird is a desktop application, not a website. Do not add custom hover
  tips, custom tooltips, or other web-style hover-only guidance.
- Keep the mouse cursor as the system default everywhere. Do not use
  `cursor: pointer` or any other cursor-changing style for buttons, links,
  draggable areas, disabled controls, or other interactive elements.

## Existing behavior first

- For integration work, read `internal-docs/integration-memory.md` when present
  (local-only feedback). Recheck current code; older notes may describe retired
  layouts or an earlier, incomplete implementation.
- Before adding or extending a tool integration, read
  `internal-docs/tool-integration-playbook.md` when present. Record the platform
  capability table before changing code.
- Before adding a feature similar to one already in the app, identify the
  reference files and trace the full interaction through handlers, effects,
  and backend calls. Compare loading, selection, refresh, errors, and saved
  state; matching the appearance alone is insufficient.
- Reuse existing components and behavior. Keep provider-specific differences
  limited to necessary authentication, storage, and API details. Do not add
  automatic actions, new interaction flows, or new frameworks unless the
  task requires them. State any necessary behavioral difference and its reason
  before implementing it.
- Tool detection and executable resolution belong in `services/tool_manager.rs`
  with catalog entries in `tools/<id>/paths.json`; Windows Store package lookup
  uses `services/msix.rs`. Launch through `services/process_manager.rs` and
  create background commands with `utils/process.rs` so Windows console
  suppression is shared. Keep explicit interactive terminal launches separate.
- Evaluate Windows, macOS, and Linux for every new integration. Separate
  vendor availability from EchoBird implementation gaps; do not silently
  restrict an existing cross-platform client to Windows. Verify native paths,
  credential storage, process handling, and installation on supported systems,
  and state any platform limitation before release.
- Before implementation, record the requested capabilities against all three
  platforms, with the existing code to reuse and the verification for each.
  Distinguish implemented, unverified, and vendor-unavailable; do not turn an
  excluded capability into new scope to fill the table.
- A visual-only request must preserve unrelated typography, spacing, layout,
  and interaction. Keep the latest user-approved result as the baseline.
- If existing implementations disagree, follow the user's confirmed behavior
  and these conventions. Do not silently choose one and call it consistent.
  Ask a focused question only when a product decision remains unresolved.
- For behavior changes, establish regression expectations from the agreed
  workflow before coding. Cover entry, exit, tool/tab switching, missing local
  state, pending requests, and failures where relevant. Assert which network
  calls, writes, and dialogs must not happen during passive navigation.
- Review against the reference behavior as well as code correctness. Passing
  tests alone does not establish consistency. Report what was reused and any
  necessary differences; keep unrelated implementations outside the change.
- Before calling an integration complete, reconcile that capability table with
  evidence for detection, installation, launch, and the requested account or
  model operations. A Windows build or UI approval does not verify other OSes
  or native account switching. Report build, fixture, and live-client checks
  separately; verify test instrumentation with a positive control.
- At a handoff or interruption, save the current goal, approved decisions,
  reference files, changed/committed state, verification limits, and next step
  in a local note. Resume from that note and current Git state; do not restart
  the design or silently drop unfinished parts when the user gives feedback.

## Local Windows builds and previews

- On this machine, keep Cargo output on D: under
  `D:/EchoBird-build/<worktree-id>/target` (use `main` for the primary checkout).
  Before compiling, configure the checkout's ignored `.cargo/config.toml`
  with `[build] target-dir` and verify Cargo metadata resolves that directory.
  Check for `CARGO_TARGET_DIR` overrides. Never fall back to a C: target.
- Reuse the same cache for that checkout; do not create a new output directory
  for each preview. Different checkouts must use separate directories.
- For frontend-only previews, reuse a verified current backend. Rebuild when
  Rust changes. Verify the executable and frontend server belong to EchoBird
  and this checkout before launching; do not launch another project's client.
- After a preview, stop only its owned processes and remove its temporary
  overrides/logs. Retain the D: cache for reuse; remove it when the checkout
  is retired, after verifying its exact path and that no process uses it.
- These are local Windows storage rules; do not add machine-specific paths
  to shared Cargo configuration or change macOS/Linux/CI build locations.

## CI gates (must pass locally before pushing)

CI runs two jobs in parallel, each with this order — **format runs first and
fails the whole job before clippy/test even start**, so a format slip hides
whether the code is actually sound. Match the order locally:

**Frontend** (`src/`, repo root):
```bash
npm run typecheck        # tsc --noEmit
npm run format:check     # prettier --check  (or `npm run format` to auto-fix)
npm run lint             # eslint  (or `npm run lint:fix`)
npm test                 # vitest
```

**Rust** (`src-tauri/`, or pass `--manifest-path src-tauri/Cargo.toml`):
```bash
cargo fmt --all --check  # FORMAT FIRST. `cargo fmt --all` to auto-fix.
cargo clippy --workspace --all-targets --all-features -- -D warnings   # CI denies warnings
cargo test --workspace
```

Run **all of them**, every push. A green `cargo check`/`cargo test` does NOT
imply a green CI — `cargo fmt --check` is a separate gate and the one most
often skipped. Same for the frontend: `npm run format:check` is not implied by
`typecheck` or `lint`.

### Lesson (learned the hard way)

Commit `4774b1d5` passed `cargo check` + `cargo clippy` + `cargo test` locally
but failed CI because `cargo fmt` hadn't been run (a braced-import reorder and
a wrapped method chain). CI's `cargo fmt --check` failed in the first step,
so clippy/test never executed — the failure said nothing about correctness,
only about formatting. Don't repeat it: treat the format gate as equal to
clippy/test, not optional.

## Verification habits

- After writing Rust: `cargo fmt --all`, then `cargo clippy --workspace --all-targets --all-features -- -D warnings`, then `cargo test --workspace`.
- After writing TS/TSX/JSON: `npm run format` (or `format:check`), `npm run typecheck`, `npm run lint`.
- The `winreg` crate's `FromRegValue for String` does **not** auto-expand
  `REG_EXPAND_SZ` — values like `%USERPROFILE%\...` come back literal. Expand
  them via `services::tool_manager::expand_path` (or `expand_path`-equivalent)
  rather than assuming the registry hands back resolved paths.
- Don't add `#![cfg(...)]` inside a module file when the `mod` declaration in
  `services/mod.rs` already gates it with `#[cfg(...)]` — rustfmt flags it as
  a duplicated attribute.

## Account login UI conventions

- In Account Center summaries, show a lone balance as a centered large
  Balance + value line, matching Model Center. Prefer a lone value for credits
  or available quota. Retain meaningful period/bucket names
  and labels needed to distinguish multiple metrics. Center a single metric in
  the card body; give two progress rows more space without adding card height.

- Reuse `src/pages/AppManager/useManagedAccounts.ts` for account loading,
  selection, login deadlines/cancellation, refresh and deletion. Provider hooks
  supply protocol/API and storage adapters; do not copy the lifecycle again.
- Use `src/pages/AppManager/AccountSectionPrimitives.tsx` for account login
  buttons and rows. ChatGPT is the structural reference; do not copy a new
  provider-specific button/row implementation.
- Keep provider colors via `colorClassName` and provider quota content via
  `secondary`. Claude, DeepSeek and WorkBuddy retain their designed colors;
  Grok uses the same black/white treatment as ChatGPT.
- Buttons are 48px tall, pill-shaped, with a 24px product icon and the existing
  add-account / waiting-for-browser translations. Embedded browser login timeout
  is 10 minutes; other login flows retain their existing 60-second timeout.
- Rows are 48px tall: 16px selection marker on the left, identity above quota
  in the center, plan above refresh/delete actions on the right. Display the
  plan once. Missing quota is unknown (`—`), never fabricated as zero.
- Use Lucide refresh/delete icons. Only show refresh when a real refresh
  action exists; disable it while pending. Clicks and keyboard actions on
  nested controls must not change account selection. Rows support Enter/Space.
- Selecting an account clears the API-model selection and vice versa. Adding
  an account saves it in EchoBird; applying a selection switches the client.
  Keep the existing delete confirmation. Cancel and clean up login on timeout
  or leaving the tool, and ignore late responses from earlier attempts.
- No extra explanatory copy, hover tips, or cursor changes. New integrations
  must test selection/action isolation, pending state and their quota display.
- For new or changed integrations, entering a tool or switching tabs/pages may
  load saved accounts for an installed client, but must not query quota, start
  login, or apply account/model changes. Quota refresh is an explicit user
  action. Uninstalled clients must not load native account configuration.
- Passive account-loading failures must preserve cached rows and must not open
  a blocking dialog. Explicit login, refresh, apply, and delete failures remain
  visible. Include these expectations in navigation regression tests.

## Release scope and worktrees

- Before tagging a release, reconcile the requested release scope with the
  active task and linked chats or worktrees. List each intended fix with its
  source branch and commit, including completed fixes still on local branches.
- Integrate the intended fixes onto current `origin/main` before tagging.
  For each fix, record its commit on main, including the resulting commit if
  cherry-picked or rebased. Surface any intended fix that cannot be included
  before publishing.
- Check that every recorded commit is an ancestor of the exact release tag
  with `git merge-base --is-ancestor <commit> <tag>`. Confirm the packaging
  run builds that tag's commit and include the verified scope in the handoff.

## Commit policy

Commit only when asked. Push only when asked. Keep history linear (rebase onto
`origin/main` when the remote has moved).
