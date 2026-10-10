# PROTOTYPE: Linear-grade UI for keefast (throwaway)

**Question:** what should keefast look like if it were as polished as linear.app?

These are throwaway HTML mockups of every screen of the desktop app, built on one shared design
system ([DESIGN.md](DESIGN.md), `kit/`). They aren't GPUI code. Once a direction wins, rewrite it
properly in `crates/app/src/` and keep this folder on its prototype branch only.

## Run

```sh
python3 -m http.server 4173 -d crates/app/prototype-linear-ui
# open http://127.0.0.1:4173/
```

Each screen has 2–3 structurally different variants. Use the floating bar at the bottom of the
page (or `←` / `→`) to switch between them, and `T` to toggle light/dark. The variant is kept in
`?variant=` so links can be shared. Some screens accept `&state=…` to show a secondary state.

## Layout

- `DESIGN.md`: the central design guidance (principles, tokens, components, patterns)
- `kit/tokens.css`, `kit/components.css`: the design system
- `kit/kit.js`: shared fake Vaults (`KF.data`), helpers, and the variant switcher
- `screens/*.html`: one file per screen
- `shots/*.png`: 1280×800 screenshots of every variant
- `index.html`: gallery of every screen and variant
