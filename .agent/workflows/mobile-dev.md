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

## PC → Mobile Sync Rules

**When `Channels.tsx` is modified, `MobileApp.tsx` MUST be updated in the same commit.**

The mobile app mirrors the PC Channels page 1:1. Any logic, guard, state, or UI change
on PC must be reflected on mobile. Use this mapping to find the corresponding mobile code:

### Module Mapping

| PC Module (`Channels.tsx`) | Mobile Module (`MobileApp.tsx`) | Notes |
|---|---|---|
| `ChannelsPanel` (server list) | `screen === 'servers'` JSX | Agent icon, status dot, hasNew, isTyping, role name |
| `ChannelsInner` (chat area) | `screen === 'chat'` JSX | Messages, loading bubble, input bar |
| `handleSend` (4-step flow) | `sendMessage` callback | detect → start → setRole → chat |
| `handleModelSelect` | Model dropdown `onClick` | modelWriting lock, rollback, anthropic protocol |
| `AgentRolePicker` | `screen === 'setup'` JSX | Agent/role selection, detection |
| `RemoteModelSelector` | `.chat-model-btn` + `.model-dropdown` | Icon-only on mobile, full dropdown |
| `allRemoteModelLoading` | `modelWriting` state | Locks input + send + shows spinner |
| `allBridgeStatus` | `connectionStatus` state | standby/connecting/connected/disconnected |
| `allBridgeHasNew` | `hasNewMessages` state | Red dot on server list |
| `remoteAgentCache` | `remoteAgentCacheRef` | Per-server agent detection cache |
| `lastAppliedRoleRef` | `lastAppliedRoleRef` | Avoid redundant setRole calls |
| `bridgeLoading` | `loading` state | Typing indicator + send lock |
| `channelModelList` filter | `doLoadModels(agentId)` | Anthropic-only for Claude Code |

### Sync Checklist (When Modifying `Channels.tsx`)

1. **Identify** which module was changed (see mapping above)
2. **Find** the corresponding code in `MobileApp.tsx`
3. **Apply** the same logic change, adapting for mobile layout:
   - Text → may need icon-only version
   - Dropdown → may open upward
   - Hover states → skip (no hover on mobile)
4. **Verify** in browser at `http://localhost:5174/?mobile=1`
5. **Check** no new `:hover` was introduced

### Common Sync Scenarios

| PC Change | Mobile Action |
|---|---|
| New guard in `handleSend` | Add same guard in `sendMessage` |
| New state variable | Add matching state in `MobileApp` |
| New API call in send flow | Add same call at same step position |
| Model selector UI change | Update `.chat-model-btn` / `.model-dropdown` |
| Server list card change | Update `screen === 'servers'` JSX |
| New error message key | Add English equivalent (mobile is EN-only for now) |
| New loading/disabled state | Mirror in mobile with same conditions |

## Checklist Before Committing Mobile Changes

- [ ] Zero `:hover` pseudo-classes in MobileApp.css
- [ ] No new text/labels/features not on PC Channels
- [ ] Colors match cyber-* palette exactly
- [ ] Safe area insets applied to top/bottom edges
- [ ] Input bar stays single-line
- [ ] Model selector is icon-only (full name in dropdown only)
- [ ] Encoding: UTF-8 no BOM, CRLF line endings

## Checklist Before Committing PC Channels Changes

- [ ] Checked module mapping — found corresponding mobile code
- [ ] Applied same logic/guard/state change to `MobileApp.tsx`
- [ ] Verified mobile at `localhost:5174/?mobile=1`
- [ ] No mobile-breaking changes introduced
