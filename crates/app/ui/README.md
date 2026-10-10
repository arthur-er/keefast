# keefast desktop UI

The web UI of keefast's Tauri app (ADR 0008): Vite, React 19, TypeScript (strict), Tailwind CSS 4,
pnpm. It runs in the Tauri window over the Rust backend in `crates/app/src/`, or in any browser
over a mock backend with fake Vaults.

The screens are being rebuilt after the Linear-style prototypes (one variant per screen, picked in
PR #83: `DESIGN.md` §7 on the `feat/linear-style-ui-prototypes` branch). What is here now is the
foundation they build on, and placeholder screens (`src/screens/`) that prove the round trip.

## Commands

Run them in `crates/app/ui` (with Nix: inside `nix develop`, which has Node, pnpm and
`cargo tauri`).

| Command | What it does |
|---|---|
| `pnpm install` | Installs the dependencies (pnpm 10, as `packageManager` says). |
| `pnpm dev:mock` | Serves the UI on <http://localhost:1421> over the **mock backend**, in any browser. |
| `pnpm dev` | Serves the UI on :1420 for the Tauri window (Tauri's `devUrl`). Opened in a plain browser, it falls back to the mock. |
| `cd .. && cargo tauri dev` | Runs the real app with the UI reloading as you edit (it starts `pnpm dev` itself). App arguments go after `-- --`: `cargo tauri dev -- -- ../../fixtures/keepassxc-kdbx4.kdbx` (master password `fixture-password`). |
| `pnpm typecheck` | `tsc --noEmit`. |
| `pnpm test` | Vitest (jsdom). |
| `pnpm build` | Typechecks and builds `dist/`, which a release build of keefast serves from inside the binary (`--features keefast/custom-protocol`). |
| `pnpm build:mock` / `pnpm preview:mock` | The UI over the mock as static files (`dist-mock/`); CI uploads it as an artifact. |
| `pnpm bindings` | Regenerates `src/bindings.ts` from the Rust commands, DTOs and events. |
| `pnpm bindings:check` | Fails if `src/bindings.ts` is stale (CI runs this test with `cargo test --workspace`). |

A plain `cargo build -p keefast` makes a dev build that loads the Vite server; release builds need
the UI built first and `--features keefast/custom-protocol`.

## Mock mode

`pnpm dev:mock` (or `pnpm dev` in a plain browser) runs everything against
`src/api/mock/`, an in-memory implementation of **every** command and event, over the prototype's
fake Vaults (`KF.data` from the prototype's `kit/kit.js`: Personal, Work in Dropbox, and Family,
locked). It is type-checked against the generated bindings, so a new Rust command doesn't compile
here until the mock answers it (`src/api/mock/mock.test.ts` also checks the names at run time).
Production builds don't include it.

- Any master password unlocks except `wrong`, which fails as a wrong master key does.
- File dialogs answer with made-up paths; copying uses the browser's clipboard and "clears" it
  after the configured wait; Dropbox shows a small fake folder tree.
- States a screen must show, from the console, agent-browser's `eval`, or the URL:

  ```js
  __keefastMock.simulate("pairing")     // a Pairing request
  __keefastMock.simulate("access")      // github.com asking for an Entry
  __keefastMock.simulate("login")       // a captured login to save
  __keefastMock.simulate("lockUnsaved") // a Vault locking with unsaved changes
  __keefastMock.simulate("merge")       // a Merge notice
  __keefastMock.simulate("saveFailed")  // a failed save
  __keefastMock.simulate("unlocked-none") // every Vault locked: Open a Vault
  __keefastMock.simulate("closed")      // Family locked, Personal and Work recent: Open a Vault B's story
  __keefastMock.reset()
  ```

  or `http://localhost:1421/?mock=access,merge`. Themes: `__keefastMock.commands.setTheme("dark")`.

When a screen needs something the mock does crudely, improve the mock in the same PR.

## Comparing with the prototype

The prototype is the reference for every screen. Get it and its screenshots without switching
branches:

```sh
git fetch origin feat/linear-style-ui-prototypes
mkdir -p /tmp/proto && git archive origin/feat/linear-style-ui-prototypes crates/app/prototype-linear-ui | tar -x -C /tmp/proto
python3 -m http.server 4173 -d /tmp/proto/crates/app/prototype-linear-ui   # the prototype, live
ls /tmp/proto/crates/app/prototype-linear-ui/shots/                         # 1280×800 screenshots
```

The chosen variants are `shots/<screen>-<variant>*.png`, e.g. `vault-browser-C.png`,
`unlock-B.png` (the table is in `DESIGN.md` §7). Then shoot the mock UI the same way, with
[agent-browser](https://github.com/vercel-labs/agent-browser) in a session of your own:

```sh
export AGENT_BROWSER_SESSION=kf-<your-screen>
agent-browser open "http://localhost:1421/?mock=access"
agent-browser set viewport 1280 800
agent-browser eval '__keefastMock.commands.setTheme("dark")'   # the prototype's shots are dark
agent-browser screenshot /tmp/mine.png
agent-browser open "http://localhost:4173/screens/dialogs.html?variant=A&state=access&shot=1"
agent-browser screenshot /tmp/theirs.png
agent-browser close
```

Look at both side by side (and at the prototype's markup: `screens/<screen>.html` is the HTML and
CSS to reproduce), in dark and in light.

## Layout

```
src/
  bindings.ts          generated by tauri-specta: commands, DTO types, events. Don't edit.
  api/
    index.ts           `api` (commands unwrapped: data or a thrown ApiError), `onEvent`, `backend`
    queries.ts         TanStack Query hooks; everything refetches on `workspaceChanged`
    backend.ts         the Backend shape both backends implement, derived from the bindings
    tauri.ts           the real backend
    mock/              the mock backend and its fake Vaults
  kit/                 React components over components.css (below)
  lib/
    theme.ts           System/Dark/Light → `data-theme` on <html>
    shortcuts.ts       global shortcuts (`useShortcut`, `SHORTCUTS`): Ctrl K palette, Ctrl Shift P alias…
    activity.ts        reports keys/clicks/scrolls to the backend for the idle lock
    utils.ts           `cn`
  styles/
    tokens.css         the prototype's design tokens, copied (dark default, `data-theme="light"`)
    components.css     the prototype's component classes, copied (`.btn`, `.input`, `.row`, `.menu`…)
    app.css            Tailwind, the bundled fonts, and the tokens as Tailwind theme values
  screens/             the placeholder screens; replace them
```

### Tokens and kit

`styles/tokens.css` and `styles/components.css` are the prototype's `kit/` files, so prototype
markup works class for class: `<button class="btn primary sm">`, `<div class="row is-selected">`,
`<kbd>`. The only changes: no remote fonts (Inter and JetBrains Mono come from
`@fontsource-variable`), and no prototype switcher. Keep them in sync with the prototype rather
than editing them; put screen CSS next to the screen or in Tailwind classes.

The tokens are also Tailwind theme values (`styles/app.css`): `bg-bg-0`, `bg-bg-1`, `bg-bg-sidebar`,
`text-fg-0/1/2`, `border-border`, `border-border-strong`, `bg-accent`, `text-accent-fg`,
`text-danger`, `bg-c-red`…, `rounded-xs/sm/md/lg` (4/6/8/12 px), `shadow-pop`, `shadow-dialog`,
`text-xs/sm/md/lg/xl` (11/12/13/15/20 px), `font-mono`, `animate-kf-pop`. The kit's classes sit in
Tailwind's `components` layer, so a utility on the same element wins (`btn h-10`).

`src/kit/` wraps the classes in React, on Radix where behaviour matters: `Button` (with a
`shortcut`), `IconButton`, `Input`/`AffixInput`/`PasswordInput`/`Textarea`/`Field`, `Kbd`,
`Badge`/`Tag`/`ColorDot`, `Tabs` (the Vault browser C tab style), `Switch`/`Check`, `Segmented`
(`.seg`), `Select`, `Menu`/`ContextMenu`, `Popover`, `Dialog`, `Tooltip`, `toast`/`Toaster`
(one at a time, with a countdown bar for clipboard clearing), and `EntryIcon` (KeePass's standard
icons as Lucide icons, custom icons, favicons).

Icons are `lucide-react`; components.css sizes `svg.lucide` (16 px, 14 in buttons, stroke 1.75).

## The command API

Everything the UI does goes through `api` (`src/api/index.ts`), typed from `src/bindings.ts`:

```ts
const snapshot = await api.getWorkspace();          // or useWorkspace()
await api.unlockVault({ provider: "local", path }, { password, keyFile: null });
await api.copyField(vault, entry, { kind: "password" }); // copied in Rust, cleared after the wait
const password = await api.revealPassword(vault, entry); // only when the user asks to see it
```

- **Snapshot and events.** `getWorkspace()` returns the shell's state: Vaults (unlocked, locking,
  locked), the view, the save status, recent Vault files, tags, and what waits for an answer
  (`pendingLock`, `pendingPairing`, `pendingAccess`, `pendingLogin`). After every change, also a
  background one (saves, syncs, Merges, idle and OS locks, browser requests), the backend emits
  `workspaceChanged` with a higher revision, and `useBackendEvents` refetches every query. Other
  events: `mergeNotice`, `browserRequest` (keefast also came to the front), `clipboardCleared`,
  `saveFailed`.
- **Where the user is** belongs to the backend: read `snapshot.view`, change it with
  `api.setView(...)`. Locking and unlocking change it too (unlocking returns the user to where
  they were).
- **Secrets.** Entry DTOs never carry passwords, protected custom fields or TOTP secrets. Use the
  `reveal*` commands when the user shows one, `copyField` to copy, and in `EntryInput` send
  `password: null` and a custom field's `value: null` (with its `original` index) to keep what is
  there without revealing it.
- **Errors.** A failed command throws `ApiError` with `kind` (`wrongKey`, `invalid`, `notFound`,
  `empty`, `alreadyOpen`, `dropbox`…), a `message` to show as is, and an optional `detail`.
- **Times** are seconds since 1970 UTC (`new Date(seconds * 1000)`); ids are numbers (Vaults,
  browser requests) or hex strings (Entries, Groups, custom icons).

### Adding or changing a command

1. Write it in `crates/app/src/commands/<domain>.rs`: `#[tauri::command] #[specta::specta] pub
   async fn ...(core: Core<'_>, ...) -> ApiResult<T>`. DTOs derive `Serialize`/`Deserialize` and
   `specta::Type` with `#[serde(rename_all = "camelCase")]`; avoid `u64`/`i64`/`f64` in DTOs
   (specta refuses 64-bit integers, and `f64` becomes `number | null`): use `u32`. Change the
   Workspace through `core.update` (or `core.edit`, which autosaves) so the UI hears of it.
2. Register it in `specta_builder()` in `crates/app/src/app.rs`.
3. `pnpm bindings`, then implement it in the mock (`pnpm typecheck` tells you it's missing).
4. Commit `src/bindings.ts` with the change.
