# UI/UX cleanup audit (pre-implementation)

Full pass over `src/` before any UI cleanup code is written. Scope: every
page, layout primitive, and reusable component that exists today. No SSH/
agent/SFTP/persistence/permission logic is touched by this audit or by the
phases that follow it - this is presentation-layer only.

Two things shape every recommendation below:

1. **There are currently zero `@media` breakpoints anywhere in the app**
   (the only media query in the whole codebase is `prefers-reduced-motion`
   in `globals.css`). Every "responsive" behavior that exists today comes
   from `flex`/`grid` reflow (`minmax()`, `flex-wrap`, `overflow-x: auto`),
   not from breakpoints. That's a reasonable foundation, not a rewrite
   target - most of it already degrades gracefully. The real gap is the
   handful of places that *don't* reflow (`.stat-grid` most notably) and
   the total absence of any layout-level breakpoint for the shell itself
   (sidebar auto-behavior, page padding).
2. **The Tauri window has `minWidth: 960` / `minHeight: 600`** (`apps/desktop/src-tauri/
   tauri.conf.json`). The brief's requested test widths of 900px and 800px
   are below what the OS window can currently reach - a user can never
   actually see the app at those sizes today. Recommendation: keep 960 as
   the real floor (matches "dense dev tool, not a mobile SaaS layout"),
   make everything solid down to 960, and treat 900/800 as a defensive
   second-tier check (things shouldn't *shatter* there, but pixel-perfect
   polish at those sizes isn't the bar) rather than a primary target. Flagging
   this rather than silently picking one - easy to revisit if you'd rather
   lower `minWidth` instead.

## What's already solid (not touching these)

Calling these out explicitly so the phases below don't waste time
"fixing" things that already work:

- **One icon system.** Every icon goes through `<Icon>` (`components/ui/
  Icon.tsx`), which wraps a bundled Lucide subset via iconify. No emoji, no
  hand-drawn SVG, no mixed libraries anywhere in `src/`.
- **One color/elevation token system.** `globals.css` defines a full
  palette + a second `--t-*` alias layer (ported from Voltius) covering
  surfaces, text, borders, rings, elevation shadows. Every component
  already consumes these tokens rather than hardcoding hex values - a spot
  check across every CSS file found no stray hardcoded colors outside
  `globals.css` itself and the three traffic-light terminal-button dots
  (`#ff5f56` etc., which are meant to be literal, not theme, colors).
- **`.servers-grid` and `.monitor-history-grid`** already use `repeat(auto-fit/
  auto-fill, minmax(...))` - they already reflow column count with window
  width. No fix needed.
- **The shared `.page` / `.card` / `.server-list` / `EmptyState` / `Badge`
  primitives** are consistently reused across every page (Dashboard,
  Servers, Monitor, Actions, Teams, ModulePicker, TeamDetail all build on
  the same handful of classes). The "every screen looks like a different
  project" feeling in the brief is really about spacing/sizing
  *inconsistencies within* that shared system (see CRITICAL/HIGH below),
  not a lack of a system.
- **Keyboard nav already exists** in the sidebar (arrow-key link cycling)
  and every icon-only button already carries `aria-label`/`title`
  (`ServerCardActionButton`, `server-list-action` buttons, modal close
  buttons, Rail buttons). Focus-visible outline is defined globally.
- **z-index is already a small, sane ladder**, not a "random 99999999"
  situation: content(0) → terminal search overlay(5) → popovers(50) →
  modal(100) → toast(200). Worth formalizing as named tokens (Phase 1) but
  not worth panicking about.

## Problem list

### CRITICAL

| # | Where | Problem |
|---|---|---|
| C1 | `pages.css` `.stat-grid` | Hardcoded `repeat(4, minmax(0,1fr))`. Dashboard's 4 stat cards do not reflow at all - at anything under ~700px of content width they visibly squash (title/value truncate or wrap awkwardly). Every other grid in the app already uses `auto-fit`/`auto-fill`; this one was missed. |
| C2 | `Rail.css` `.rail-instances` clipping (fixed this session, verified) | Was clipping the instance hover tooltip via `overflow-x: hidden`. **Already fixed** in this session (portal + debounced hover) - listing here only so it isn't "rediscovered" as new during the responsive pass. |
| C3 | `Actions.tsx` service/container list rows | `server-list-actions` (badges + 3-4 icon buttons) is `flex-shrink: 0` with no fallback; at the 960px floor with a long service name, the name column (`min-width:0`, correctly shrinkable) can be squeezed to just a few characters before anything else gives. Not literally broken (nothing overlaps), but at the edge of "unusable" - needs a documented minimum before it's called done. |

### HIGH

| # | Where | Problem |
|---|---|---|
| H1 | `pages.css` `.page-title` | 20px/700. Brief's own target (and reasonable practice) is 24-28px for a page title - current hierarchy between page title (20px) and card title (14px) is only 6px apart, weak visual hierarchy. |
| H2 | No sidebar auto-behavior | Sidebar (200px expanded) never auto-collapses at narrow window widths - only a manual toggle. At 960px floor with sidebar expanded (200px) + Rail (52px), content area is ~700px, which is fine, but there's no signal to the user that collapsing helps on a cramped window. |
| H3 | `ServerCard.css` `.server-card-name` / `.server-card-host` | Ellipsis-truncate with no `title` attribute fallback - a long hostname or server name has no way to be read in full without opening the server (Files/Terminal). Same gap in `MonitorPage`'s process `command` column (has `max-width:320px` + ellipsis, no title) and Rail's tooltip name (already has `overflow:hidden` truncation, no title either). |
| H4 | Modal width is ad-hoc per-modal | `AddServerModal`/`CreateEntryModal`/`AuthModal`/`TeamDetail` tabs all use the CSS default (520px), but `Actions.tsx`'s confirm dialog inlines `style={{width:420}}`, `ContainerLogsPanel` inlines `style={{width:720}}`, `DeleteServerDialog` inlines `style={{width:400}}`. Four different ad-hoc widths for what should be 2-3 named size variants (sm/md/lg). |
| H5 | Icon-only "small button" is reimplemented ~6 times | `.server-list-action` (28px), `.server-card-action` (padding:6, no fixed box), `.rail-btn` (36px), `.terminal-tab-close` (16px), `.terminal-search-btn` (padding:4), `.modal-close` (28px), `.toast-dismiss` (20px) are all independent copies of the same "square icon button, centered, radius, hover-bg, color transition" pattern with different sizes and slightly different hover treatments. Not broken individually, but it's the concrete cause of "buttons have inconsistent heights" in the brief. |
| H6 | `Button` has no `size` prop | Only `variant` (primary/secondary/ghost/danger) exists; every button is 36px tall. Combined with H5, there's no shared small-button primitive at all - every "sm" button in the app is a bespoke class. |

### MEDIUM

| # | Where | Problem |
|---|---|---|
| M1 | Repeated inline `style={{...}}` blocks | `Actions.tsx` (confirm modal: width, body-text style, gap), `DeleteServerDialog.tsx` (identical body-text style + gap), `ContainerLogsPanel.tsx` (width + a ~13-property inline `<pre>` block + gap), `SshServerForm.tsx` (`marginTop:8`, `justifyContent:"space-between"`, error color), `ModulePicker.tsx` (flex centering). All magic numbers that belong in CSS classes. `.form-actions` itself has no `gap` defined in `forms.css` (the 3 copies of `style={{gap:8}}` are each independently patching that gap - pun intended). |
| M2 | `ServersSection.css` `.team-servers-port-input { flex: 0 0 80px !important; }` | Only non-reduced-motion `!important` in the app; a specificity fight it shouldn't need to win with. |
| M3 | Font-size floor inconsistently applied | 10-11.5px used for a mix of genuinely decorative eyebrows (sidebar group headers, badges - fine at that size given uppercase+letterspacing) and borderline-data labels (`.server-card-latency` 11px, `.metrics-gauge-header`/`.metrics-history-chart-header` 11px, `.monitor-process-table th` 11px). None of these are unreadable, but a couple (gauge/chart headers specifically) are real data labels, not eyebrows, and read cramped next to 13-14px body text elsewhere. |
| M4 | `Card` header has no action slot | Every page that needs a header-level action (RolesSection's "Create role", Teams' create form) puts it in the card *body* instead - consistent today, but worth an explicit `actions` prop so it's a pattern instead of a convention every page has to remember. |
| M5 | Settings page has no information architecture | Only 2 Cards (Preferences, About) exist today - the brief's category-sidebar ask is solving a problem that doesn't exist yet at this app's actual feature count. Flagging as "don't build IA for options that don't exist" rather than a bug - revisit once Settings actually grows past ~2 sections. |
| M6 | `docs/` convention says nothing about a spacing/type scale | No documented scale to point new work at - Phase 1 needs to write one down, not invent tokens in participants' heads. |

### LOW

| # | Where | Problem |
|---|---|---|
| L1 | Sidebar collapsed-mode tooltip uses native `title` | Works, but is the same "OS tooltip clashes with dark theme" issue already fixed for Rail's instance tooltip this session. Lower priority since it's plain text, not sensitive data - polish only. |
| L2 | `.rail-btn`/`.server-list-action` active-state `transform: scale()` micro-interaction exists inconsistently | Some buttons (`server-list-action`, `rail-btn`) have a press-scale, others (`server-card-action`, `terminal-search-btn`) don't. Minor. |
| L3 | Skeleton loading heights are hand-picked per call site (`SkeletonRows count={3} height={52}` etc.) | Reasonable given varied row heights, but worth double-checking each still roughly matches its real content's height during Phase 4-6 so the loading→loaded transition doesn't visibly jump. |

## Design tokens (proposed)

Nothing here replaces the existing color/elevation tokens in `globals.css`
- this only adds the two scales that don't exist yet, plus formalizes the
z-index ladder that already exists informally.

**Spacing scale** (new custom properties, additive to `globals.css`):
```
--space-1: 4px;
--space-2: 8px;
--space-3: 12px;
--space-4: 16px;
--space-5: 20px;
--space-6: 24px;
--space-8: 32px;
--space-10: 40px;
--space-12: 48px;
```
Existing values overwhelmingly already land on this scale (4/8/12/16/20/24
show up constantly; 31px/17px-style off-scale values are rare - the audit
above found none worth calling out individually). This is mostly about
giving the scale a name so new code has something to reach for, not a
mass find-replace.

**Typography scale** (additive):
```
--text-xs:   12px;  /* secondary metadata - timestamps, hints */
--text-sm:   13px;  /* body default, table cells, buttons */
--text-md:   14px;  /* card titles, emphasized body */
--text-lg:   16px;  /* section titles */
--text-xl:   20px;  /* current page title - see H1 */
--text-2xl:  26px;  /* proposed new page-title size, see H1 */
```
`--text-2xl` (26px) becomes the new `.page-title` size in Phase 3, closing
H1. Nothing currently below 12px will be used for real data after Phase
3-6 land (M3) - decorative uppercase eyebrows (10.5px sidebar/badge labels)
are the one deliberate exception and stay as-is, since letter-spacing +
uppercase is what makes 10.5px legible there, not a target for the body
scale.

**z-index scale** (formalizing what already exists):
```
--z-content: 0;
--z-sticky: 5;      /* terminal search overlay today */
--z-popover: 50;    /* Rail/MemberRolesEditor popovers */
--z-modal: 100;
--z-toast: 200;
```

**Breakpoint strategy**: one shell-level breakpoint, not a full grid
system, since this is a desktop app with a 960px floor, not a marketing
site:
```
--bp-compact: 1100px;  /* below this: sidebar suggests collapsing, page
                           padding steps down from 24px to 16px */
```
`.stat-grid`, `.servers-grid`, `.monitor-history-grid` etc. stay
content-driven (`auto-fit`/`auto-fill` `minmax()`) rather than keying off
this breakpoint directly - that's already the right pattern (C1's fix is
adopting it, not introducing a new mechanism).

## Component normalization plan

1. **`IconButton` primitmive** (new, `components/ui/IconButton.tsx` +
   `.css`) - closes H5/H6. Sizes `sm` (28px, today's `.server-list-action`)
   and `md` (36px, today's `.rail-btn`). Replaces the bespoke classes one
   call site at a time in Phases 4-6; `.terminal-tab-close`/`.toast-
   dismiss` stay bespoke (they're smaller, single-purpose chrome, not
   reusable actions) rather than forcing every 16-20px control into one
   component.
2. **`Button` gets a `size` prop** (`sm`/`md`, default `md`) - closes the
   other half of H6. `sm` reuses the same 28px height as `IconButton`'s
   `sm` so a text button and an icon button never mismatch height when
   they sit side by side (Actions.tsx confirm-dialog buttons, page-header
   secondary actions).
3. **Modal size variants**: `modal-panel-sm` (400px), default (520px,
   unchanged), `modal-panel-lg` (720px) in `AddServerModal.css`. Closes H4.
   `DeleteServerDialog`→sm, `ContainerLogsPanel`→lg, `Actions.tsx` confirm→sm.
4. **`.form-actions { gap: 8px }`** added once in `forms.css`, all three
   inline `style={{gap:8}}` copies deleted. Closes half of M1.
5. **`.dialog-body-text` class** in `forms.css` for the repeated "13px/
   line-height 1.5/text-primary" paragraph style - replaces the 2 identical
   inline copies (`Actions.tsx`, `DeleteServerDialog.tsx`). Closes the rest
   of the P1-relevant part of M1; `ContainerLogsPanel`'s `<pre>` block gets
   its own `.container-logs-output` class instead of a 13-property inline
   style.
6. **`title` attribute on truncated text** - `ServerCard`'s name/host,
   Rail tooltip name, Monitor's process-command cell all get `title={full
   value}` so a native tooltip covers the truncated case. Closes H3. (A
   custom styled tooltip isn't warranted here the way it was for Rail's
   host-masking case - there's no privacy concern with these particular
   values, so the plain native title is the right amount of engineering.)
7. **`Card` gains an optional `actions` node**, rendered right-aligned in
   `.card-header` next to title/subtitle. Existing call sites keep working
   unchanged (prop is optional); new/touched call sites in Phases 4-7 adopt
   it where it removes a body-level action row. Addresses M4.

## Phase order

Matches the brief's phase numbering; each phase ends with build + typecheck
+ existing-test pass and a list of touched files before moving on.

1. **Global foundation** - add the spacing/typography/z-index tokens and
   `--bp-compact` to `globals.css`. No visual change yet, just the
   vocabulary the rest of the phases use.
2. **Sidebar + navigation + PageLayout** - `.stat-grid` fix (C1), sidebar
   width/collapse behavior at the compact breakpoint (H2), page padding
   step-down below `--bp-compact`.
3. **Typography + buttons + inputs + shared components** - `.page-title`
   to 26px (H1), `IconButton` primitive + `Button` size prop (H5/H6),
   `.form-actions` gap + `.dialog-body-text` (M1), modal size variants (H4).
4. **Dashboard + Servers** - adopt `IconButton`/modal variants here first
   (smallest, most self-contained pages); `title` attrs on `ServerCard`
   (H3).
5. **Terminal + Files** - verify `xterm.fit()` on window/sidebar-collapse
   resize (behavioral check, not styling); adopt shared primitives.
6. **Processes/Services/Docker/Monitoring** (`Actions.tsx`, `MonitorPage`)
   - row-density pass for C3, `title` on process command (H3), chart/gauge
   label size bump (M3), modal-size adoption for the confirm dialog and
   `ContainerLogsPanel`.
7. **Settings** - low-touch given M5; mainly absorbs the typography/button
   token changes from Phase 3, no IA rework.
8. **Remaining screens** - Teams/TeamDetail/RolesSection/ServersSection/
   MemberRolesEditor, AuthModal, ModulePicker: token/primitive adoption
   pass, no structural changes expected (already consistent per the "what's
   already solid" section).
9. **Responsive regression pass** - walk every screen at 1920/1600/1440/
   1366/1280/1024/960 (and spot-check 900/800 per the note at the top of
   this doc) and record findings inline in this file under a new "Phase 9
   results" section.
10. **Visual consistency pass** - second pass focused only on: page
    titles, card headers, button/badge/icon sizing, spacing rhythm, border/
    radius consistency across everything touched in Phases 2-8.

No Theme Creator page exists in this codebase today (the brief describes
one) - Phase 7 will not invent it; if you want it built, that's a new
feature and belongs in a separate request, not this cleanup pass.

## Phase 9 results (responsive regression pass)

Walked Dashboard, Servers, Terminal, Files, Monitor, Actions, Teams,
TeamDetail (all 3 tabs), Settings at 1024x768, 900x650, and 800x600 (the
latter two below the real 960px window floor, checked for defensive
robustness per the note at the top of this doc) using two seeded servers
(one with a deliberately long name) and a simulated signed-in user.

- No overlapping elements found on any screen at any tested width.
- `.stat-grid` (C1 fix) reflows correctly down to 1 column at 800px.
- Files toolbar (3 buttons, now `size="sm"` from Phase 5) stays on one line
  with the breadcrumb even at 800px.
- A long server name at 800px wraps the page `<h1>` to two lines rather
  than overflowing or truncating - the header row's flex layout keeps the
  "Back" button from colliding with it. Acceptable degradation for a width
  below the real floor; not treated as a bug.
- **Found and fixed one new bug during this pass**: `.modal-tab-active`
  (the tab-bar component now shared between modals and `TeamDetail`'s
  `.page-tabs`) lost its accent color while the mouse hovered the
  already-active tab - `.modal-tab:hover`'s color rule has higher CSS
  specificity than `.modal-tab-active`'s, so hovering the current tab made
  it visually look inactive. Pre-existing bug (same CSS existed before this
  cleanup pass), not something introduced by Phases 1-8, but caught here
  because Phase 8 was the first time this tab bar got hovered during
  testing. Fixed with an explicit `.modal-tab-active:hover` rule in
  `AddServerModal.css` - verified via computed-style check
  (`getComputedStyle().color` reads back the accent teal both hovered and
  not).

Build ✅ typecheck ✅ after every fix in this pass.
