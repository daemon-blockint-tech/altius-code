# Anthropic/Claude Design System for Altius Fleet PWA

Re-skin the Fleet PWA (`crates/altius-cli/assets/pwa/`) with the Anthropic/Claude token system — warm terracotta accent, Poppins + Lora typography, simplified single-column layout with a top bar replacing the sidebar.

## Files Changed

| File | Change |
|---|---|
| `assets/pwa/design-system.css` | Full token swap (color, typography, spacing, shadows) + layout restructure (sidebar → topbar, single-column) |
| `assets/pwa/index.html` | New HTML structure (topbar + single-column), Google Fonts `<link>` tags, relaxed CSP for fonts, updated `theme-color` |
| `assets/pwa/app.js` | Update DOM element IDs to match new HTML (remove sidebar refs, add topbar refs) |
| `assets/pwa/manifest.webmanifest` | Update `theme_color` and `background_color` to `#141413` |

## Step 1 — `design-system.css`: Token Swap

### Color tokens (light mode `:root`)

| Token | Old | New | Source |
|---|---|---|---|
| `--bg-base` | `#f7f9fc` | `#faf9f5` | Anthropic Light |
| `--bg-raised` | `#ffffff` | `#ffffff` | keep (cards pop on warm bg) |
| `--bg-subtle` | `#eef2f7` | `#f0eee5` | warm subtle |
| `--bg-subtlest` | `#f4f6fa` | `#f5f3ec` | warm subtlest |
| `--bg-quiet` | `#e6ebf2` | `#e8e6dc` | Anthropic Light Gray |
| `--bg-offset` | `#dce3ed` | `#d8d5c8` | warm offset |
| `--bg-inverse` | `#172033` | `#141413` | Anthropic Dark |
| `--fg` | `#172033` | `#141413` | Anthropic Dark |
| `--fg-quiet` | `#55637a` | `#b0aea5` | Anthropic Mid Gray |
| `--fg-quieter` | `#616c7e` | `#c4c2b8` | lighter mid gray |
| `--fg-inverse` | `#ffffff` | `#faf9f5` | Anthropic Light |
| `--border-subtlest` | `#e4e9f0` | `#e8e6dc` | Anthropic Light Gray |
| `--border-subtle` | `#d4dbe6` | `#d8d5c8` | warm border |
| `--border-moderate` | `#aeb9c9` | `#b0aea5` | Anthropic Mid Gray |
| `--border-focus` | `#1769e0` | `#d97757` | Anthropic Orange |
| `--accent` | `#1769e0` | `#d97757` | Anthropic Orange (Primary) |
| `--accent-hover` | `#1257bd` | `#c46843` | darker terracotta |
| `--accent-soft` | `#e7f0fd` | `#f5e6dd` | warm orange tint |
| `--ok` | `#137a4a` | `#788c5d` | Anthropic Green |
| `--ok-soft` | `#e2f5eb` | `#eef0e8` | green tint |
| `--warn` | `#9a5d00` | `#9a5d00` | keep (works with warm palette) |
| `--warn-soft` | `#fff1d5` | `#fff1d5` | keep |
| `--bad` | `#b42335` | `#b42335` | keep (red reads well on warm) |
| `--bad-soft` | `#fde8eb` | `#fde8eb` | keep |

### Color tokens (dark mode — both `@media` and `[data-theme="dark"]`)

| Token | Old | New |
|---|---|---|
| `--bg-base` | `#0b101b` | `#141413` |
| `--bg-raised` | `#121927` | `#1c1b18` |
| `--bg-subtle` | `#171f2f` | `#22211d` |
| `--bg-subtlest` | `#101725` | `#1a1916` |
| `--bg-quiet` | `#202a3d` | `#2a2823` |
| `--bg-offset` | `#29354a` | `#353330` |
| `--bg-inverse` | `#edf3ff` | `#faf9f5` |
| `--fg` | `#edf3ff` | `#faf9f5` |
| `--fg-quiet` | `#aeb9cc` | `#b0aea5` |
| `--fg-quieter` | `#7f8da5` | `#8a887e` |
| `--fg-inverse` | `#0b101b` | `#141413` |
| `--border-subtlest` | `#202a3b` | `#2a2823` |
| `--border-subtle` | `#2c384d` | `#353330` |
| `--border-moderate` | `#46546c` | `#5a574e` |
| `--border-focus` | `#71a8ff` | `#d97757` |
| `--accent` | `#71a8ff` | `#d97757` |
| `--accent-hover` | `#8db9ff` | `#e08868` |
| `--accent-soft` | `#172b4b` | `#3a2a22` |
| `--ok` | `#57d69a` | `#9caf7a` |
| `--ok-soft` | `#143528` | `#2a3322` |
| `--warn` | `#f0bd5a` | `#f0bd5a` |
| `--warn-soft` | `#3c2e13` | `#3c2e13` |
| `--bad` | `#ff8897` | `#ff8897` |
| `--bad-soft` | `#401e27` | `#401e27` |
| `--shadow-card` | blue-tinted | `0 1px 2px rgb(0 0 0 / 15%), 0 8px 24px rgb(0 0 0 / 12%)` |

### Typography tokens

| Token | Old | New |
|---|---|---|
| `--font-sans` (body) | system-ui stack | `"Lora", Georgia, "Times New Roman", serif` |
| `--font-display` (NEW) | — | `"Poppins", Arial, sans-serif` |
| `--font-mono` | (existing) | keep unchanged |

### Shadow tokens (light)

`--shadow-card`: change from blue-tinted to warm-neutral: `0 1px 2px rgb(20 20 19 / 6%), 0 8px 24px rgb(20 20 19 / 5%)`

### Remove `--sidebar-width` token (no longer needed)

## Step 2 — `design-system.css`: Layout Restructure

### Remove
- `.app-shell` (flex sidebar + main)
- `.sidebar`, `.sidebar-nav`, `.sidebar-footer`, `.sidebar-meta`, `.sidebar-history`, `.sidebar-heading`
- `--sidebar-width` token
- Mobile `@media` rules for `.sidebar` and `.sidebar-nav`

### Add
- `.topbar` — sticky top bar: brand left, nav pills center, theme toggle + meta right. Flex row, `border-bottom: 1px solid var(--border-subtlest)`, `backdrop-filter: blur(8px)`, `background: color-mix(in srgb, var(--bg-base), transparent 20%)`
- `.topbar-brand` — Poppins, semibold, `font-size: 1.125rem`
- `.topbar-nav` — inline-flex gap of nav pills
- `.topbar-meta` — right-aligned, `font-size: var(--text-xs)`, `color: var(--fg-quieter)`
- `.topbar-actions` — right side: theme toggle button

### Update
- `.main` — remove `flex: 1`; becomes the single content container with `max-width: 720px; margin: 0 auto; padding: var(--size-xl)`
- `.main-inner` — remove `max-width` and `margin` (moved to `.main`)
- `.page-heading` — add `font-family: var(--font-display)`
- `.brand` → `.topbar-brand` — add `font-family: var(--font-display)`
- `.section-heading` — add `font-family: var(--font-display)`
- `.approval-title` — add `font-family: var(--font-display)`
- `.nav-item` — restyle as horizontal pill (inline-flex, auto width, `border-radius: var(--radius-full)`)
- `.nav-item.active` — `background: var(--accent-soft); color: var(--accent)`
- `.history-list` / `.history-item` — keep but restyle for inline use under the dispatch view (collapsible section in main content, not sidebar)
- Mobile `@media (max-width: 800px)` — simplify: topbar stays as a row (brand + toggle only), nav pills wrap below; remove sidebar-specific rules

## Step 3 — `index.html`: New Structure

### `<head>` changes
1. Update `theme-color` to `#141413`
2. Relax CSP: add `fonts.googleapis.com` to `style-src`, `fonts.gstatic.com` to `font-src` (new directive)
3. Add Google Fonts preconnect + stylesheet links:
   ```html
   <link rel="preconnect" href="https://fonts.googleapis.com" />
   <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
   <link href="https://fonts.googleapis.com/css2?family=Lora:ital,wght@0,400;0,500;0,600;1,400&family=Poppins:wght@600;700&display=swap" rel="stylesheet" />
   ```

### `<body>` restructure

**Replace** the `<div class="app-shell">` + `<aside class="sidebar">` + `<main class="main">` with:

```
<header class="topbar">
  <h1 class="topbar-brand">Altius Fleet</h1>
  <nav class="topbar-nav">
    <button id="nav-new" class="nav-item">+ New</button>
    <button id="nav-dispatch" class="nav-item active" data-view="dispatch">Dispatch</button>
    <button id="nav-runs" class="nav-item" data-view="runs">Runs</button>
  </nav>
  <div class="topbar-actions">
    <span class="topbar-meta">localhost · no auth</span>
    <button id="theme-toggle" class="btn btn-ghost btn-pill btn-icon">
      <span id="theme-icon">◐</span>
    </button>
  </div>
</header>

<main class="main">
  <div class="main-inner home" id="main-inner">
    <!-- dispatch view (unchanged content) -->
    <!-- runs view (unchanged content) -->
  </div>
</main>
```

**Move** the history list (`sidebar-history` / `history-list`) into the dispatch view section, above the composer, as a collapsible row of recent run chips.

**Keep** all existing element IDs that `app.js` references (`prompt`, `send`, `send-icon`, `refresh`, `error`, `runs`, `detail`, `detail-id`, `detail-status`, `detail-body`, `approval`, `approval-title`, `approval-detail`, `approval-msg`, `approve`, `cancel`, `agent-pills`, `agent-eyebrow`, `theme-toggle`, `theme-icon`, `nav-new`, `nav-dispatch`, `nav-runs`, `main-inner`, `history-list`).

**Remove** the `sidebar-history` wrapper div and `sidebar-meta` paragraph (meta moves to topbar).

## Step 4 — `app.js`: DOM Reference Updates

1. **Remove** `sidebarHistory` and `sidebarMeta` from `els` object
2. **Update** `renderHistory()` — query `#history-list` directly (it now lives in the dispatch view); toggle visibility based on runs.length using the parent container's `hidden` attribute
3. **Update** the `sidebarMeta` text-setting block (lines 516-520) — target `.topbar-meta` instead:
   ```js
   const meta = document.querySelector(".topbar-meta");
   if (meta) meta.textContent = authToken ? "localhost · bearer auth" : "localhost · no auth";
   ```
4. All other event listeners (`navNew`, `navDispatch`, `navRuns`, `themeToggle`, `send`, etc.) remain unchanged — the IDs are preserved in the new HTML

## Step 5 — `manifest.webmanifest`

- `theme_color`: `#0b101b` → `#141413`
- `background_color`: `#0b101b` → `#141413`

## Verification

1. `cargo build --locked --release -p altius-cli` — ensure it still compiles (assets are static, no Rust changes)
2. Run `./target/release/altius fleet serve --offline` and open `http://127.0.0.1:8788/app/` in a browser
3. Check: warm off-white background, terracotta accent on send button and active nav, Poppins headings, Lora body text
4. Toggle theme (system → light → dark) — dark mode should show `#141413` background with `#d97757` accent
5. Dispatch a run, verify approval card renders with warm warn-soft background
6. Check mobile viewport: topbar collapses, nav pills wrap, single column
