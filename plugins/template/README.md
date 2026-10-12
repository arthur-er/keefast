# The template Plugin

A keefast Plugin for bank accounts, to start your own from: a facet kept in the custom fields
`IBAN`, `BIC` and `Account holder`, an IBAN formatted in groups of four, its check digits
validated, and labelled "DE •••• 3000".

- `plugin.json`: the manifest.
- `src/lib.rs`: the hooks, written with `keefast-plugin-sdk`, and their tests.

Copy this directory, swap the SDK's `path` dependency in `Cargo.toml` for the `git` one, and follow
[the guide](../../docs/plugins/README.md#quick-start):

```sh
cargo test
keefast-plugin keygen ~/keys/my-plugin.key     # once; put its publisherKey in plugin.json
keefast-plugin build
keefast-plugin pack --key ~/keys/my-plugin.key -o my-plugin.kfplugin
keefast-plugin verify my-plugin.kfplugin
```

CI builds, packs and verifies it, so it keeps working with the SDK.
