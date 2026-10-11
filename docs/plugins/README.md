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
| `keefast-plugin-sdk` (`crates/plugin-sdk`) | The SDK: the host API's bindings, the `Plugin` trait and helpers. Built into your component. | Your Plugin's crate depends on it. |
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

API version 1 has one contribution kind: **facets**. Later versions add Entry list subtitles,
command palette commands and browser fill, each as a new key under `contributions`.

### Facets

A facet is a kind of data an Entry can have, such as a Card. keefast finds it on an Entry from
the Entry's custom fields, keeps it in the facet's Owned fields, and shows it as a section in the
Entry detail, drawn with keefast's own kit.

| Key | What it is |
|---|---|
| `id` | Unique within the Plugin: lowercase letters, digits and `-`. Hooks are told which facet they're called for. |
| `title` | The section's title, in sentence case. |
| `icon` | Optional. One of the icons keefast draws: `credit-card`, `landmark`. Any other name shows a generic Plugin icon. |
| `detect` | When an Entry has the facet: `{ "fields": [names…] }`, when it has a custom field of each of these names. They must be among the facet's own fields. |
| `fields` | The facet's Owned fields, in the order the section shows them. At least one. |

Each Owned field:

| Key | What it is |
|---|---|
| `key` | How hooks name it: lowercase letters, digits and `-`, unique within the facet. |
| `name` | The custom field's name in the Vault file, such as `Card number`. Readable by people: it's what other KeePass apps show. Unique across the Plugin. |
| `label` | What the section calls it. |
| `kind` | `text`; `digits` (typed on a number pad, shown in a monospace font); or `monthYear`, a month kept as `MM/YY`, such as a card's expiry, of which keefast says when it has passed or is near. |
| `protected` | Optional, `false` by default. Whether the value is kept protected, like a password: masked until revealed, and not given to the Plugin's hooks unless it has the `readProtected` Plugin capability. |

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
hook the `facet` id, the Owned field's `key` and the `value`; later API versions may tell it more,
so build one with `Input::new` in tests.

| Hook | keefast calls it | It returns | Default |
|---|---|---|---|
| `format(input)` | when the user types a value into an Owned field | the value as the field keeps it: digits in groups, a slash in a date | the value as typed |
| `validate(input)` | when the user edits an Owned field | what's wrong with the value, if anything: `Problem::error(…)` keeps it from being saved, `Problem::warning(…)` doesn't | nothing wrong |
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
| `copy` | Have keefast offer to copy its Owned fields. keefast does the copying, and clears the clipboard as it does for passwords. |

Ask only for what the Plugin needs: users see the list.

## The sandbox

keefast runs each Plugin's component in [wasmtime](https://wasmtime.dev), inside keefast's own
process, sandboxed:

- **No network, no filesystem, no clock, no randomness.** The component may import nothing at
  all: one that imports anything, such as WASI, doesn't load. So build for
  `wasm32-unknown-unknown`, not `wasm32-wasip1` or `wasm32-wasip2`. In Rust's standard library on
  that target, files, sockets and threads return errors, and `SystemTime::now()` panics. If a hook
  needs to know what time it is, the host has to pass it in, as a later API version may.
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
  `Cargo.lock`; `build` uses it (`--locked`) when it's there.
- `pack` signs the manifest, component and icon into a package, after checking keefast would
  install it: the manifest reads, the component loads in keefast's sandbox, and the id isn't
  keefast's. In CI, pass the key in an environment variable with `--key-env`, from a secret.
- `verify` checks a package as keefast does when installing it, and prints its id, version,
  publisher, key fingerprint, digest, Plugin capabilities and Owned fields. With
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
