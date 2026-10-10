# The Plugin package (`.kfplugin`), version 1

A Plugin is installed from one file, its package: the manifest, the WASM component and an icon,
signed with its publisher's ed25519 key (ADR 0009). keefast reads it with
`keefast_plugins::Package::read` (`src/package.rs`) and makes one with
`keefast_plugins::package::pack`, or from the command line:

```sh
cargo run -p keefast-plugins --example pack -- new-key publisher.key   # prints the publisherKey
cargo run -p keefast-plugins --example pack -- pack publisher.key plugin.json plugin.wasm icon.png acme.iban.kfplugin
```

## Container

An **uncompressed tar archive** (ustar or GNU), no larger than **10 MiB**, holding regular files at
its top level and nothing else:

| Entry | Needed | At most | What it is |
|---|---|---|---|
| `plugin.json` | yes | 64 KiB | The manifest, UTF-8 JSON (below). |
| `plugin.wasm` | yes | 8 MiB | The WebAssembly component, of the `plugin` world in `wit/plugin.wit`. |
| `icon.png` | no | 256 KiB | The Plugin's icon, a PNG, shown in Settings → Plugins. SVG isn't accepted: it can hold script. |
| `signature` | yes | 64 bytes | The ed25519 signature (below), raw. |

The entries may come in any order. keefast refuses a package that has:

- any other entry, including directories, links and devices;
- a name that starts with `/`, contains `\`, or has a `..` part (refused as pointing outside the
  package, before anything else is said of it);
- an entry twice;
- an entry larger than its limit, a file over 10 MiB, or anything that isn't a tar archive;
- an `icon.png` that doesn't start with PNG's signature.

## Manifest

`plugin.json` is the manifest `src/manifest.rs` reads, strictly: an unknown key, field kind or
Plugin capability is refused rather than ignored. A package's manifest must also name its
publisher:

```json
{
  "id": "acme.iban",
  "name": "IBAN",
  "version": "1.0.0",
  "publisher": "Acme Ltd",
  "publisherKey": "6ZxR0dIhgvRWh0iu0p1Qb0fF2sv6S8ZsJ1yF1wN0d0k=",
  "apiVersion": 1,
  "capabilities": ["copy"],
  "contributions": { "facets": [ … ] }
}
```

- `publisher`: who made it, as they call themselves. keefast can't check a name, so it is shown as
  unverified.
- `publisherKey`: the publisher's ed25519 public key, 32 bytes in standard base64.
- Ids starting `keefast.` are keefast's own; a package with one is refused.

## Signature

The signature signs this text, in UTF-8, with the secret key matching `publisherKey`:

```
keefast-plugin-package-v1\n
then, for each entry but `signature`, by name in byte order:
<name>\n<size in bytes, decimal>\n<SHA-256 of its bytes, lowercase hex>\n
```

For example:

```
keefast-plugin-package-v1
icon.png
1270
9f2c…
plugin.json
612
4b0e…
plugin.wasm
38211
c71d…
```

keefast checks it with ed25519's strict verification against the `publisherKey` the manifest
names. Since the manifest is among what is signed, so is the key: a package whose contents, key or
signature were changed after signing is refused. The SHA-256 of the signed text is the package's
**digest**: it names exactly one package, and installing checks that the package installed is the
one whose Plugin capabilities the user was shown.

## Trust

There is no authority vouching for publishers. The first package keefast installs for a Plugin id
pins its key (trust on first use), and the publisher is shown as unverified. Every later package for
that id, updates included, must be signed by the same key, or it is refused. Removing the Plugin
forgets the key.

Signatures are checked when a package is installed and again each time keefast loads it.

## Where installed Plugins live

In keefast's local data directory, `keefast/plugins/` (`~/.local/share/keefast/plugins` on Linux,
`%LOCALAPPDATA%\keefast\plugins` on Windows):

- `<id>.kfplugin`: the package as installed, checked again at every load;
- `plugins.json`: for each installed Plugin, whether it is enabled, the Plugin capabilities and
  Owned fields the user agreed to, and the publisher key pinned for it; and which of keefast's own
  Plugins the user disabled.

A package that fails its checks at load, or asks for more than the user agreed to, doesn't run,
and Settings → Plugins says why.
