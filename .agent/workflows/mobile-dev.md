---
description: Mobile development rules — must read before any mobile (MobileApp.tsx/css) changes
---

# Mobile Development Workflow

The mobile app (`src/mobile/MobileApp.tsx` + `MobileApp.css`) is a **vertical skin of the PC Channels page**.
It shares the same Tauri backend, SSH config, Bridge protocol, CDN roles, and Model Nexus.

## Core Principles

### 1. No Hover Effects
Mobile devices have no cursor. **Never add `:hover` pseudo-classes** in `MobileApp.css`.
- No `color` change on hover
- No `background` change on hover
- No `border-color` change on hover
- No `transform` / `filter` on hover
- Keep `:active` / `:focus` only if needed for tap feedback

### 2. Strict 1:1 Channel Page Skin
The mobile UI is a **wrapper around the PC Channels page logic** — not a new product.
- **Do NOT add** new features, text, labels, or UI elements that don't exist on PC
- **Do NOT add** placeholder text, empty state messages, or instructional copy
- **Do NOT invent** new interactions — mirror the PC Channels behavior
- **OK to optimize** layout for vertical mobile viewport (e.g. collapse text to icon-only)

### 3. Space Optimization
Mobile screens are narrow. Optimize for space:
- Model selector: **icon-only** in the input bar (dropdown still shows full names)
- Server names: truncate with `text-overflow: ellipsis`
- Role names in header: truncate, show full name only in setup screen
- Input bar: single-line layout `[📎] [input] [model-icon] [▶]`

### 4. Safe Areas
Always respect mobile safe areas (notch, home indicator):
```css
padding-top: env(safe-area-inset-top, 0px);
padding-bottom: env(safe-area-inset-bottom, 0px);
```
These are invisible in web browser dev tools but critical on real devices.

### 5. Color Scheme
Use the **exact same cyber-* palette** from `tailwind.config.js`:
| Token | Hex | Usage |
|-------|-----|-------|
| cyber-bg | `#0F1117` | Page background |
| cyber-terminal | `#131620` | Headers, input box |
| cyber-surface | `#1A1F2B` | Cards, elevated areas |
| cyber-border | `#2D3448` | Borders, dividers |
| cyber-accent | `#00FF9D` | Active states, CTAs |
| cyber-text | `#E0F7FA` | Primary text |
| cyber-text-muted | `#A8B5C8` | Secondary text |

Fonts: `JetBrains Mono` (labels, mono), `Inter` (body text).

### 6. Background
Chat area uses the same grid background as PC:
```css
background-image:
    linear-gradient(rgba(40, 46, 63, 0.2) 1px, transparent 1px),
    linear-gradient(90deg, rgba(40, 46, 63, 0.2) 1px, transparent 1px);
background-size: 40px 40px;
```

## File Structure

| File | Purpose |
|------|---------|
| `src/mobile/MobileApp.tsx` | Component logic (mirrors Channels.tsx) |
| `src/mobile/MobileApp.css` | Vanilla CSS (no Tailwind, no hover) |
| `src/App.tsx` | Routes `?mobile=1` to MobileApp |

## Dev Testing

// turbo
```
npx vite --port 5174 --force
```
Open `http://localhost:5174/?mobile=1` in browser DevTools mobile viewport (375x812).

## Checklist Before Committing Mobile Changes

- [ ] Zero `:hover` pseudo-classes in MobileApp.css
- [ ] No new text/labels/features not on PC Channels
- [ ] Colors match cyber-* palette exactly
- [ ] Safe area insets applied to top/bottom edges
- [ ] Input bar stays single-line
- [ ] Model selector is icon-only (full name in dropdown only)
- [ ] Encoding: UTF-8 no BOM, CRLF line endings
