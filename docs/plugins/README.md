# Writing a keefast Plugin

A Plugin teaches keefast a kind of data kept on Entries: payment cards, bank accounts, Wi-Fi
networks, passports. Its data stays in plain custom fields, so a Vault file stays readable in
KeePassXC, KeePass2 and KeeWeb, and the data shows as ordinary custom fields wherever the Plugin
isn't installed. keefast's own Card Plugin (`plugins/card/`) is written the way described here,
with the same SDK and the same package checks as yours.

A Plugin is three things, packed and signed into one `.kfplugin` file the user installs from
Settings → Plugins:

- a **manifest**, `plugin.json`: who the Plugin is, what it asks to do, and what it contributes;
- a **WebAssembly component** with its hooks, written in Rust with the SDK;
- optionally an **icon**, `icon.png`.

This guide is enough to build one without reading keefast's source.
[ADR 0009](../adr/0009-third-party-plugins.md) explains why Plugins work this way.

## Contents

- [The crates](#the-crates)
- [Quick start](#quick-start)
- [The manifest](#the-manifest)
- [Contributions](#contributions)
- [The hooks](#the-hooks)
- [Plugin capabilities](#plugin-capabilities)
- [The sandbox](#the-sandbox)
- [Packaging and signing](#packaging-and-signing)
- [Testing](#testing)
- [API versions](#api-versions)

Also: [the package format](package.md), and [how keefast signs its own Plugins](signing.md).

## The crates

Four crates in keefast's repository deal with Plugins. As a Plugin author you use the first two:

| Crate | What it is | Who uses it |
|---|---|---|
| `keefast-plugin-sdk` (`plugins/sdk`) | The SDK: the host API's bindings, the `Plugin` trait and helpers. Built into your component. | Your Plugin's crate depends on it. |
| `keefast-plugin-cli` (`crates/plugin-cli`) | The `keefast-plugin` command: `keygen`, `build`, `pack`, `verify`. | You run it. |
| `keefast-plugin-package` (`crates/plugin-package`) | The `.kfplugin` format: reading, signing and checking packages. | The command, and keefast. |
| `keefast-plugins` (`crates/plugins`) | keefast's Plugin host: runs Plugins in wasmtime, installs and checks packages. | keefast only. |

The names say which side each is on: `-sdk` is what a Plugin is written with, `-cli` the command
that packs it, and the plain plural `keefast-plugins` is keefast's own crate, which holds all the
Plugins it runs. The SDK depends on none of keefast's crates.

## Quick start

You need Rust (stable) with the `wasm32-unknown-unknown` target:

```sh
rustup target add wasm32-unknown-unknown
cargo install --git https://github.com/arthur-er/keefast keefast-plugin-cli
```

1. **Copy the template**, `plugins/template/` in keefast's repository: a Plugin for bank
   accounts (IBAN, BIC, account holder). In its `Cargo.toml`, swap the SDK's `path` dependency for
   the `git` one commented above it.
2. **Make your key**, once, and keep the file secret and backed up. Every update of your Plugin
   must be signed with it: keefast refuses an update signed by another key.

   ```sh
   keefast-plugin keygen ~/keys/my-plugin.key
   ```

   Put the `publisherKey` it prints in `plugin.json`.
3. **Edit** `plugin.json` (its `id`, `name`, `publisher` and facets) and the hooks in
   `src/lib.rs`, then run its tests: `cargo test`.
4. **Build** the component. It is written to `target/plugin.wasm`:

   ```sh
   keefast-plugin build
   ```

5. **Pack and sign** it, then check the package as keefast will:

   ```sh
   keefast-plugin pack --key ~/keys/my-plugin.key --icon icon.png -o my-plugin.kfplugin
   keefast-plugin verify my-plugin.kfplugin
   ```

6. **Install** it in keefast from Settings → Plugins → Install from a file.

## The manifest

`plugin.json` is JSON, read strictly: an unknown key, field kind or Plugin capability is refused
rather than ignored, so keefast never runs a Plugin that asks for something it doesn't understand.

```json
{
  "id": "example.iban",
  "name": "Bank account",
  "version": "0.1.0",
  "publisher": "Example",
  "publisherKey": "3BauPpD1aEw6alAOlhbYo9PpvcsAEtMLf8S5rsRZ2rA=",
  "apiVersion": 1,
  "capabilities": ["copy"],
  "contributions": {
    "facets": [
      {
        "id": "bank-account",
        "title": "Bank account",
        "icon": "landmark",
        "detect": { "fields": ["IBAN"] },
        "fields": [
          { "key": "iban", "name": "IBAN", "label": "IBAN", "kind": "text" },
          { "key": "bic", "name": "BIC", "label": "BIC", "kind": "text" }
        ]
      }
    ]
  }
}
```

| Key | Needed | What it is |
|---|---|---|
| `id` | yes | Unique among Plugins, reverse-domain style: lowercase letters, digits, `-` and `.`, starting with a letter, at most 64 characters. Ids starting `keefast.` are keefast's own. |
| `name` | yes | What users know it as. One line, at most 64 characters. |
| `version` | yes | Free text, such as `1.2.0`. One line, at most 64 characters. keefast shows it, and doesn't compare versions. |
| `publisher` | yes, to be packed | Who made it, as you call yourself. keefast can't check a name, so it shows it as unverified, beside your key's fingerprint. |
| `publisherKey` | yes, to be packed | Your ed25519 public key, base64, from `keefast-plugin keygen`. `pack` adds it if it's missing, but name it yourself, so packing with another key by mistake fails. |
| `apiVersion` | yes | The host API version the Plugin is written against: `1`. |
| `capabilities` | no | The [Plugin capabilities](#plugin-capabilities) it asks for. |
| `contributions` | no | What it adds to keefast: see below. |

## Contributions

API version 1 has one contribution kind: **facets**. A facet brings its own sections, its Entry
list subtitle, its command palette commands, and the web forms the Companion extension fills from
it.

### Facets

A facet is a kind of data an Entry can have, such as a Card. keefast finds it on an Entry from
the Entry's custom fields, keeps it in the facet's Owned fields, and shows it as a section in the
Entry detail, drawn with keefast's own kit. With an `editor`, it is also a section of the Entry
editor, which the user adds to an Entry and takes off it again.

| Key | What it is |
|---|---|
| `id` | Unique within the Plugin: lowercase letters, digits and `-`. Hooks are told which facet they're called for. |
| `title` | The section's title, in sentence case. |
| `icon` | Optional. One of the icons keefast draws: `credit-card`, `landmark`. Any other name shows a generic Plugin icon. |
| `detect` | When an Entry has the facet: `{ "fields": [names…] }`, when it has a custom field of each of these names. They must be among the facet's own fields. |
| `fields` | The facet's Owned fields, in the order the section shows them. At least one. |
| `editor` | Optional. `{ "add": "Add card", "remove": "Remove card" }`: the facet has a section in the Entry editor, after Login, and these name the actions that add it beside "+ Field" and take it off, with its Owned fields. Without it, the Entry editor shows the facet's Owned fields as ordinary custom fields. |
| `subtitle` | Optional, `false` by default. `true` shows the facet's label from `derive` under the Entry's title in the Entry list, such as "Visa •••• 1111", when the Entry has no username. A username always wins. With several such facets on an Entry, the first, in the order Plugins load, that has a label is shown. |
| `commands` | Optional. [Commands](#commands) in the command palette's Entry section, for the selected Entry when it has the facet. |
| `fill` | Optional. The web forms the Companion extension fills from the facet: see [Browser fill](#browser-fill). |

Each Owned field:

| Key | What it is |
|---|---|
| `key` | How hooks name it: lowercase letters, digits and `-`, unique within the facet. |
| `name` | The custom field's name in the Vault file, such as `Card number`. Readable by people: it's what other KeePass apps show. Unique across the Plugin. |
| `label` | What the section calls it. |
| `kind` | `text`; `digits` (typed on a number pad, shown in a monospace font); or `monthYear`, a month kept as `MM/YY`, such as a card's expiry, of which keefast says when it has passed or is near. |
| `protected` | Optional, `false` by default. Whether the value is kept protected, like a password: masked until revealed, and not given to the Plugin's hooks unless it has the `readProtected` Plugin capability. |
| `placeholder` | Optional. What the field's input shows while empty in the Entry editor, such as `MM/YY`. |

In the Entry editor, a facet's section saves the fields it is detected by even while empty, so
the facet stays on the Entry, and its other fields once they have a value. Protected values the
user doesn't reveal never leave keefast's Rust side.

### Commands

Each command copies one of the facet's Owned fields from the selected Entry. keefast does the
copying, Rust-to-clipboard, and clears the clipboard as it does for passwords, so a command needs
the `copy` Plugin capability: a manifest with commands and without `copy` is refused.

```json
"commands": [
  { "id": "copy-number", "title": "Copy card number", "copy": "number", "shortcut": "Ctrl Shift C" }
]
```

| Key | What it is |
|---|---|
| `id` | Unique within the facet: lowercase letters, digits and `-`. |
| `title` | As the command palette lists it, in sentence case: "Copy card number". |
| `copy` | The `key` of the Owned field it copies. |
| `shortcut` | Optional. The keys that run it, written as keefast writes them: `Ctrl Shift C`. |

The command is listed only when the Entry has a value to copy, and its keys never fire while the
user is typing in a field. keefast checks a shortcut when it loads the Plugin, and refuses **the
shortcut, not the Plugin**, when:

- it isn't **Ctrl with a letter**, **Ctrl Shift with a letter**, or **Ctrl with a digit**. Those
  never type text and read the same on every keyboard layout; Alt, Meta, named keys and Shift
  with a digit don't;
- keefast has it (`crates/app/ui/src/lib/shortcuts.json`: Ctrl K, Ctrl B, Ctrl Shift N…), or text
  fields do (Ctrl A, C, V, X, Y, Z, Ctrl Shift Z);
- a Plugin loaded before yours has it, or another of your commands does. Bundled Plugins load
  first, then installed ones in the order they were installed. A Plugin enabled again loads
  last. When a Plugin is disabled or removed, the others get their shortcuts again, so one it
  held is free.

A command without its shortcut is still in the command palette. Settings → Plugins says, on the
Plugin's row, which shortcuts it didn't get and why. `keefast-plugin pack` and `verify` warn
about a shortcut keefast or its own Plugins would refuse. Pick keys that are likely free, and say
in your Plugin's description what they are.

### Browser fill

With `fill`, the Companion extension can fill some of the facet's Owned fields into a kind of web
form, for an Entry the user picks there. It needs the `browserFill` Plugin capability: a manifest
with `fill` and without it is refused. The Card fills payment forms:

```json
"fill": {
  "form": "payment",
  "fields": { "number": "cc-number", "holder": "cc-name", "expiry": "cc-exp", "cvv": "cc-csc" }
}
```

| Key | What it is |
|---|---|
| `form` | The kind of form: `payment`, the only one in API version 1. |
| `fields` | Each Owned field filled, by `key`, and the HTML [`autocomplete` token](https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#autofill-field) of the form field it goes into. A payment form's are `cc-number`, `cc-name`, `cc-exp`, `cc-exp-month`, `cc-exp-year` and `cc-csc`. Owned fields left out are never filled: the Card leaves out its PIN. |

keefast refuses a manifest whose `fill` names a field the facet doesn't own, a token it doesn't
know or that isn't the form's, or one token twice. `cc-exp` is filled from a `monthYear` field;
the extension writes it as the page's field wants it (`08/31`, `08/2031`, a month and a year
apart, or a `<select>`). The extension decides where each value goes, from the page's
`autocomplete` attributes and, for some fields, their names and labels. A `cc-csc` value only ever
goes into a field the page marked `cc-csc`.

keefast never runs your Plugin to fill: the values go to the browser as the Vault keeps them.
Which sites get them follows the user's Access decisions, as logins do.

**Owned fields are exclusive.** keefast refuses to install a Plugin owning a field another
installed Plugin owns, enabled or not. Choose names that say what they hold, so they don't clash
by accident: `Card number`, not `Number`.

## The hooks

The component exports three hooks. Each is a pure function: it gets values and returns values,
and keefast does all the reading and writing of the Vault. With the SDK, you implement the
`Plugin` trait and call `export!` once:

```rust
use keefast_plugin_sdk::{Derived, Fields, Input, Plugin, Problem};

struct BankAccount;

impl Plugin for BankAccount {
    fn format(input: &Input) -> String { /* … */ }
    fn validate(input: &Input) -> Option<Problem> { /* … */ }
    fn derive(facet: &str, fields: &Fields) -> Derived { /* … */ }
}

keefast_plugin_sdk::export!(BankAccount);
```

Each hook has a default that does nothing, so implement only the ones you need. `Input` tells a
hook the `facet` id, the Owned field's `key` and the `value`, and `validate` also `today`: the
month it is now, in UTC, as a `MonthYear` (`None` in `format`). A Plugin has no clock, so this is
how it says a date has passed or is near. Later API versions may tell a hook more, so build an
`Input` with `Input::new` in tests, and `.with_today(MonthYear { year: 2026, month: 10 })` to test
`validate` on a given month.

| Hook | keefast calls it | It returns | Default |
|---|---|---|---|
| `format(input)` | in the Entry editor, as the user types into an Owned field | the value as the field keeps it: digits in groups, a slash in a date. keefast keeps the caret after the same letters and digits, so add only separators | the value as typed |
| `validate(input)` | in the Entry editor, on each value the user types or reveals | what's wrong with the value, if anything: `Problem::warning(…)` is shown under the field and the Entry still saves; `Problem::error(…)` keeps it from being saved until the value changes. Prefer warnings: the user may know better | nothing wrong |
| `derive(facet, fields)` | when an Entry with the facet is shown | `Derived::new().label(…)` for a short label of the facet on this Entry, such as "Visa •••• 1111"; `.text(key, …)` for text shown in place of a field's value | nothing |

A problem's message is shown as is: write one sentence saying what to do, such as "This IBAN has
a typo: its check digits don't match." keefast cuts it at 200 characters.

`Fields` reads the facet's Owned fields on one Entry by key, typed by kind:

- `fields.text("holder")`: the value, or `None` when the Entry hasn't the field, it is empty, or
  it is protected and the Plugin can't read protected values;
- `fields.digits("number")`: a `digits` field without the spaces `format` put in;
- `fields.month_year("expiry")`: a `monthYear` field as a `MonthYear { year, month }`.

A label and field texts are shown without a reveal, so they must **never be a whole protected
value**: "Visa •••• 1111", not the card number. keefast shows one line of at most 64 characters of
each.

keefast calls hooks only with a facet id from your manifest, but check it if your Plugin has more
than one facet. A hook can fail: it panics, or runs out of memory or fuel. When `derive` fails, the
facet still shows, without a label or texts.

## Plugin capabilities

A Plugin can do only what the user agreed to when installing it. Before installing anything,
keefast lists what the package asks for, in plain words: its Owned fields first, then each
capability. An update that asks for more marks what is new, and keefast won't run a package
asking for more than the user agreed to.

| Capability | What it lets the Plugin do |
|---|---|
| *(its Owned fields)* | Keep its data in the custom fields its facets declare. Not listed under `capabilities`: the facets declare them. |
| `readProtected` | Get the values of its protected Owned fields in `derive`, so it can label them ("Visa •••• 1111"). Without it, a protected field's value is `None`. |
| `copy` | Have keefast offer to copy its Owned fields, from the facet's section and from its [commands](#commands). keefast does the copying, and clears the clipboard as it does for passwords. |
| `browserFill` | Have the Companion extension fill the Owned fields a facet's [`fill`](#browser-fill) names into web forms of its kind. The user sees which fields, and an update that fills one more asks again, even with `browserFill` agreed to already. |

Ask only for what the Plugin needs: users see the list.

## The sandbox

keefast runs each Plugin's component in [wasmtime](https://wasmtime.dev), inside keefast's own
process, sandboxed:

- **No network, no filesystem, no clock, no randomness.** The component may import nothing at
  all: one that imports anything, such as WASI, doesn't load. So build for
  `wasm32-unknown-unknown`, not `wasm32-wasip1` or `wasm32-wasip2`. In Rust's standard library on
  that target, files, sockets and threads return errors, and `SystemTime::now()` panics. What a
  hook needs to know of the time, keefast passes in: `validate`'s `input.today`.
- **16 MiB of memory**, at most, per Plugin. Growing past it traps.
- **Fuel per hook call**: 20 million units, roughly tens of millions of WebAssembly instructions,
  a few milliseconds' work. A hook that runs out traps. Formatting or labelling a few fields takes
  a tiny fraction of that.
- **One instance per Plugin**, kept between calls, and thrown away when a call traps; the next
  call starts a fresh one. Don't rely on state kept between calls.
- **No access to the Workspace.** A hook sees only the values of one facet's Owned fields on one
  Entry, and returns text for keefast to show or values for it to write. It never sees other
  fields, other Entries or the Vault.
- **No JavaScript, no custom UI.** keefast draws everything a Plugin contributes with its own
  kit. A Plugin can't draw custom visuals.

A package is at most 10 MiB, its component at most 8 MiB, its manifest 64 KiB and its icon
256 KiB. Keep the component small: the template's release profile (`opt-level = "s"`, LTO,
`panic = "abort"`, stripped) gives tens of kilobytes.

## Packaging and signing

`keefast-plugin` makes and checks packages:

```text
keefast-plugin keygen <secret key file>
keefast-plugin build [<plugin dir>] [-o <component.wasm>]
keefast-plugin pack (--key <secret key file> | --key-env <VAR>) [--manifest plugin.json]
                    [--component <plugin dir>/target/plugin.wasm] [--icon icon.png] -o <out.kfplugin>
keefast-plugin verify <package.kfplugin> [--publisher-key <base64>]
```

- `keygen` writes a new ed25519 secret key, readable only by you, and prints its public half for
  `publisherKey`. It never replaces an existing file.
- `build` builds the crate for `wasm32-unknown-unknown` in release mode and makes the component
  keefast runs. It is **reproducible**: paths on your machine are remapped out of the binary, so
  the same source, `Cargo.lock` and toolchain give the same bytes on any machine. Commit
  `Cargo.lock`; `build` uses it (`--locked`) when it's there. A crate you depend on by `path`
  must be in your Cargo workspace for that: Cargo identifies one outside it by its absolute path,
  which changes the component's bytes, and `build` warns of it.
- `pack` signs the manifest, component and icon into a package, after checking keefast would
  install it: the manifest reads, the component loads in keefast's sandbox, and the id isn't
  keefast's. In CI, pass the key in an environment variable with `--key-env`, from a secret.
- `verify` checks a package as keefast does when installing it, and prints its id, version,
  publisher, key fingerprint, digest, Plugin capabilities, Owned fields and the fields it fills
  into web forms, with their `autocomplete` tokens. With
  `--publisher-key`, it also checks the package is signed by that key.

**Trust.** No authority vouches for publishers. When a user first installs your Plugin, keefast
shows you as an unverified publisher, with your key's fingerprint, and pins your key for your
Plugin's id (trust on first use). Every later package for that id must be signed by the same key.
**If you lose your key, users must remove your Plugin before they can install a package signed by
a new one.** Publish your key's fingerprint where users can check it.

keefast checks a Plugin's signature when it is installed and again every time keefast starts. A
package changed on disk doesn't run.

The format is in [package.md](package.md).

## Testing

Hooks are plain Rust functions, so test them natively with `cargo test`, as the template does:

```rust
#[test]
fn the_label_shows_the_country_and_last_four() {
    let fields = Fields::of([("iban", Some("DE89 3704 0044 0532 0130 00"))]);
    let derived = BankAccount::derive("bank-account", &fields);
    assert_eq!(derived.label.as_deref(), Some("DE •••• 3000"));
}
```

`keefast-plugin pack` and `verify` load the built component in keefast's sandbox, so they catch a
component that imports something or doesn't match the host API. To try the Plugin, install the
package in keefast.

## API versions

The host API is a [WIT](https://component-model.bytecodealliance.org/design/wit.html) world,
`keefast:plugin@1.0.0`, in `crates/plugins/wit/plugin.wit`; the SDK generates its bindings from
it. A Plugin declares the version it is written for in `apiVersion`, and keefast runs Plugins of
the version it supports. A later API version will add hooks, contribution kinds and capabilities,
and may change what a hook is told. The SDK keeps that change small: `Input` is built with
`Input::new` and can gain fields without breaking your code, and every hook has a default.

The SDK isn't on crates.io yet: depend on it from git, pinned to a tag or revision for
reproducible builds.
