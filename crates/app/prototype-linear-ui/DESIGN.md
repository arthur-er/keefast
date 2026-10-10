# keefast — design direction (PROTOTYPE)

> Throwaway. This file is the central design guidance every prototype screen in this folder
> follows. It replaces the KeeWeb-derived look of `crates/app/src/theme.rs` for the purpose of
> the prototype only. Once a direction wins, rewrite it properly into the GPUI app.

The bar is **Linear**: quiet, dense, fast, keyboard-first, with a few moments of craft. A
password manager earns trust by looking calm and precise. Nothing should shout except danger.

---

## 1. Principles

1. **Content is the interface.** Chrome recedes: hairline borders, no heavy boxes, no
   gradients on surfaces. The Entry list and the Entry detail carry the screen.
2. **One accent, used sparingly.** The accent marks *the* primary action, focus rings, and the
   current selection indicator. Never decorate with it.
3. **Dense but breathable.** 13px base text, 32px rows, 4px grid. Whitespace comes from
   alignment and rhythm, not from padding everything to 24px.
4. **Keyboard-first, mouse-friendly.** Every primary action shows its shortcut (`<kbd>`) in
   a menu, tooltip, or button. The command palette (`Ctrl K`) reaches everything.
5. **Secrets look like secrets.** Passwords and TOTP codes use the mono face, are masked by
   default, and copy on click with an inline confirmation — not a modal, not a toast storm.
6. **Inline over modal.** Edit in place, popovers anchored to their trigger, side sheets over
   centred dialogs. Centred modal dialogs are only for decisions that block (unlock, confirm
   delete, a browser asking for access).
7. **Motion explains, never decorates.** 120–180ms, `cubic-bezier(.2,.8,.2,1)`. Popovers
   scale from their anchor (0.97→1) with a fade. Nothing bounces. Respect reduced motion.
8. **Dark first, light equal.** Design in dark; check every screen in light
   (`data-theme="light"` on `<html>`, toggled with `T` in the prototype switcher).

## 2. Window

- Desktop app. Design for **1280×800**, must survive 1024×680.
- The body *is* the window. No fake OS titlebar; a 40px app **toolbar** row sits at the top
  of the main area (draggable region in the real app).
- Main layout (canonical, variants may challenge it):
  `sidebar 232px | entry list 340px | detail flex` — separated by 1px `--border` lines, all
  three on `--bg-0`/`--bg-1`. The sidebar is one step darker (`--bg-sidebar`).

## 3. Tokens (see `kit/tokens.css` — always use the variables, never raw hex)

| Role | Token | Dark | Notes |
|---|---|---|---|
| Window background | `--bg-0` | `#0d0e10` | list + detail |
| Sidebar | `--bg-sidebar` | `#0a0b0c` | |
| Raised surface | `--bg-1` | `#141518` | cards, inputs, popovers base |
| Hover | `--bg-hover` | white 4% | rows, ghost buttons |
| Active / selected row | `--bg-active` | white 7% | |
| Popover / menu | `--bg-popover` | `#17181c` | + `--shadow-pop` |
| Border | `--border` | white 7% | hairlines |
| Strong border | `--border-strong` | white 12% | inputs, buttons |
| Text | `--fg-0` | `#ededef` | titles, values |
| Secondary text | `--fg-1` | `#a0a1a8` | labels, usernames |
| Tertiary text | `--fg-2` | `#6b6d76` | hints, timestamps, placeholders |
| Accent | `--accent` | violet-fuchsia | brand, derived from the icon's `#c026d3` |
| Danger / warn / ok | `--danger` `--warn` `--ok` | | text + 12% tints (`--danger-bg` …) |

Colour labels (Entry colours): `--c-red --c-orange --c-yellow --c-green --c-blue --c-purple`.

**Type.** Inter (UI), JetBrains Mono (secrets, codes, paths, kbd).

| Use | Size / weight / tracking |
|---|---|
| Page title (Entry title in detail) | 20px / 600 / -0.015em |
| Section title | 13px / 600 |
| Body / rows | 13px / 400 |
| Secondary / labels | 12px / 400, `--fg-1` |
| Micro labels (sidebar section heads, column heads) | 11px / 500, `--fg-2`, no uppercase |
| Mono secrets | 13px JetBrains Mono, `letter-spacing: .02em` |

No ALL-CAPS labels. Sentence case everywhere, using the GLOSSARY terms (Vault, Entry, Group,
Master key, Key file, Pairing, Access decision…). Capitalise those terms as the app does.

**Spacing.** 4px grid: 4 8 12 16 20 24 32. **Radii.** 4 (kbd, badges), 6 (buttons, inputs,
rows), 8 (popovers, cards), 12 (dialogs). **Icons.** Lucide, 16px, stroke 1.75, `--fg-1`;
14px inside buttons.

## 4. Components (see `kit/components.css`)

- **Buttons** `.btn` (32px) / `.btn.sm` (26px): `.primary` (accent, one per view),
  `.secondary` (bg-1 + strong border), `.ghost` (no border), `.danger`. Icon-only `.btn.icon`.
  A trailing `<kbd>` inside a button is allowed and encouraged.
- **Inputs** `.input` 32px, bg-1, strong border, focus = accent ring
  (`--ring`). `.input.mono` for secrets. Field groups: `.field > label + .input + .hint`.
- **Segmented control** `.seg` for 2–4 exclusive options (themes, generator mode, policy).
- **Switch** `.switch` for booleans. **Checkbox** `.check`. Never ☑/◉ glyphs.
- **Rows** `.row` 32px, radius 6, hover `--bg-hover`, selected `.is-selected` = `--bg-active`
  + `--fg-0`. Lists never use full-bleed accent selection like the current app.
- **Sidebar items** `.nav-item` 28px with icon, label, trailing count in `--fg-2`.
- **Badges** `.badge` (neutral), `.badge.accent|danger|warn|ok`; **tags** `.tag` with a
  coloured dot.
- **Kbd** `<kbd>` small mono key caps.
- **Menus / popovers** `.menu` + `.menu-item` (28px, icon, label, trailing kbd),
  `.menu-sep`, `.menu-label`.
- **Dialogs** `.dialog` (radius 12, `--shadow-dialog`) over `.scrim`. Header = title +
  one-line description; footer = right-aligned actions, primary last, `Esc`/`Enter` hints.
- **Sheet** `.sheet` slides from the right, 440px, for editors and history.
- **Toast** `.toast` bottom-centre, one at a time, with an optional countdown bar
  (clipboard clearing).
- **Entry avatar** `KF.avatar(entry, size)` — a rounded-square monogram tile tinted from the
  site, stand-in for favicons.
- **Empty states**: one line icon, one sentence, one action. No illustrations.

## 5. Patterns specific to keefast

- **Copy**: clicking a secret value copies it; the value briefly shows `Copied` with a check,
  and a toast shows "Password copied · clears in 30s" with a countdown bar.
- **Masking**: `•` dots in mono; an eye icon reveals; reveal is per-field and resets when
  the Entry changes.
- **TOTP**: 6-digit code split `123 456` in mono, with a small circular countdown ring.
- **Password strength**: a 4-segment bar under generated/edited passwords + entropy bits in
  `--fg-2`.
- **Status**: save/sync status lives in the toolbar as a small dot + word (`Saved`,
  `Unsaved`, `Saving…`, `Synced`, `Offline`). Unsaved is `--warn`, errors `--danger`.
- **Locked Vault**: the Vault stays in the sidebar with a lock icon; selecting it shows a
  centred unlock card in place of list+detail.
- **Shortcuts** (Linux/Windows notation, the app's primary targets): `Ctrl K` palette,
  `Ctrl F` / `/` search, `Ctrl N` new Entry, `Ctrl B` copy username, `Ctrl C` copy password,
  `Ctrl T` copy TOTP, `E`/`Enter` edit, `Ctrl S` save, `Ctrl L` lock all, `Ctrl ,` settings,
  `↑ ↓` move, `Esc` close/back.

## 6. Prototype rules for every screen

- One HTML file per screen in `screens/`, loading `../kit/*.css` and `../kit/*.js`.
- Use `KF.data` (shared fake Vaults) so every screen tells the same story.
- Register 2–3 **structurally different** variants with `KF.variants({...})`; the floating
  switcher (←/→, `T` for theme) is added automatically. Variants differ in layout,
  hierarchy and primary affordance — not colour.
- Static/in-memory only. Interactions that matter (copy, reveal, tab switches, popovers
  opening) should work; nothing persists.
- Use tokens and kit classes. Screen-specific CSS goes in a `<style>` in the screen file and
  must still use tokens only.

## 7. Chosen direction (verdict, 2026-10-10)

The user picked one variant per screen. These are the reference for the GPUI implementation;
the other variants stay on this branch for reference only.

| Screen | Pick | Prototype |
|---|---|---|
| Open a Vault | **B**: Vault files rail + unlock pane | `screens/unlock.html?variant=B` |
| Vault browser | **C**: Vault switcher, grouped list, tabbed detail | `screens/vault-browser.html?variant=C` |
| Entry editor | **A**: edit in place | `screens/entry-editor.html?variant=A` |
| Password generator | **A**: popover on the field | `screens/password-generator.html?variant=A` |
| TOTP | **B**: TOTP as a field, setup expands inline | `screens/totp.html?variant=B` |
| Entry history | **B**: full-window split (versions, diff) | `screens/entry-history.html?variant=B` |
| Command palette | **A**: classic centred, grouped sections | `screens/command-palette.html?variant=A` |
| General Settings | **C**: one long page, section index, search | `screens/settings.html?variant=C` |
| Vault settings | **C**: same style as General Settings C | `screens/vault-settings.html?variant=C` |
| Open from Dropbox | **A**: file browser dialog | `screens/dropbox.html?variant=A` |
| Browser requests & confirms | **A**: centred dialogs | `screens/dialogs.html?variant=A` |

**One shell.** Each screen was prototyped separately, so their sidebars differ in the details.
When implementing, every screen uses the **Vault browser C** shell: a Vault switcher at the top of
the sidebar, then Groups, Tags, Colours, Other Vaults, and a footer with Open / New, settings and
lock. Entry editor A, Entry history B and TOTP B render inside that shell's detail area. The
command palette, generator popover and centred dialogs float over it. General and Vault settings
replace the list and detail with the settings page and keep the sidebar.

**Consequences for the design rules above:**
- Edit in place (Entry editor A) and inline TOTP setup (TOTP B) confirm "inline over modal".
- Centred dialogs stay reserved for blocking decisions: browser requests, delete, lock with
  unsaved changes, and the Dropbox file browser.
- Settings use one scrolling page with an "On this page" index and `/` search, for both General
  and Vault settings.
