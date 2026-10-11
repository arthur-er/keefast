# How keefast signs its own Plugins

keefast's own Plugins, such as the Card Plugin, are signed packages like anyone's, and keefast
loads them through the same checks ([ADR 0009](../adr/0009-third-party-plugins.md): no private
shortcut). This page is for keefast's maintainers: the keys, where they live, and how to make and
rotate them.

## Two kinds of key

**keefast's release key** signs the Plugins bundled with official releases, and any Plugin keefast
publishes on its own. Its public half is committed in
[`crates/plugins/keys/release.pub`](../../crates/plugins/keys/release.pub) and compiled into every
build of keefast. Its secret half is only ever the GitHub Actions secret
**`KEEFAST_PLUGIN_SIGNING_KEY`**, used by one step of the release workflows. A package signed by a release key
is keefast's: it may have a `keefast.` id, and Settings shows it as keefast's rather than
unverified.

**A build's own key** signs the bundled Plugins of every other build: `cargo build`, `cargo tauri
dev`, CI's test builds, `nix build`, anyone building from source. `crates/plugins/build.rs` makes
it from the OS's randomness when it signs, and never writes its secret half anywhere, so nothing
can sign with it afterwards. (So such builds aren't byte-for-byte reproducible: each signs with a
new key.) keefast trusts it only for the packages compiled into that same
binary, never for a package installed from a file. Settings shows those Plugins as "keefast dev
build", with the key's fingerprint.

No private key is committed to this repository, real or for tests.

## What is checked, and what it protects

At every start, keefast reads each bundled package exactly as it reads an installed one: the tar
container strictly, the ed25519 signature (`verify_strict`) against the key the manifest names,
then the manifest. It also checks the key is the one the build signed with, and the id is
keefast's. A package that fails doesn't run, and Settings says why.

For the bundled Plugins, this guards the build pipeline rather than the user's disk: anyone able
to change keefast's binary could change the key compiled into it too. What it buys:

- keefast's Plugins go through the same code as everyone's, so a bug in that path shows up in
  keefast's own Plugins first;
- an official release's bundled Plugins are exactly the packages signed with the release key;
- the release key can sign Plugins keefast publishes outside a release, such as an update of a
  Plugin that isn't bundled, and every keefast build will recognise them.

## The release build

`.github/workflows/release.yml` runs the Linux and Windows packaging workflows with
`release: true`, passing them the secret. Each then:

1. builds `keefast-plugin` (`cargo build --release -p keefast-plugin-cli`), without the secret;
2. signs the bundled Plugins with it, in a step of its own, the only one given
   `KEEFAST_PLUGIN_SIGNING_KEY`: `scripts/sign-bundled-plugins.sh` packs each `plugins/<name>/`
   into `target/signed-plugins/<name>.kfplugin`;
3. builds keefast with `KEEFAST_SIGNED_PLUGINS` naming that directory, and
   `KEEFAST_REQUIRE_RELEASE_SIGNING=1`. `build.rs` compiles in each package only if it is signed by
   the **first** key in `release.pub` and holds exactly the committed `plugin.json` and component,
   and fails rather than fall back to a build's own key.

The secret never reaches `cargo build` of keefast, so no dependency's build script or procedural
macro can read it. So a release can't ship Plugins signed by anything but the current release key.
Until the owner creates the key and the secret, release builds fail at step 2; nothing else
changes. Pull requests and hand-run packaging workflows don't get the secret.

## Making the release key

Once, on a trusted machine:

```sh
cargo run -p keefast-plugin-cli -- keygen keefast-release.key
```

It prints the public key (`publisherKey: …`) and its fingerprint. Then:

1. Add the public key as the first non-comment line of `crates/plugins/keys/release.pub`, and
   commit it.
2. Create the repository secret `KEEFAST_PLUGIN_SIGNING_KEY` (Settings → Secrets and variables →
   Actions) with the contents of `keefast-release.key`, or
   `gh secret set KEEFAST_PLUGIN_SIGNING_KEY < keefast-release.key`.
3. Store `keefast-release.key` offline (a password manager, or keefast itself), then delete the
   file. GitHub won't show a secret again.
4. Publish the fingerprint somewhere users can check it, such as the README or the release
   notes.

To sign a keefast Plugin outside a release, pack it with the release key, in a workflow step
given the secret, with a `keefast-plugin` built in an earlier step:
`keefast-plugin pack --key-env KEEFAST_PLUGIN_SIGNING_KEY …`.

## Rotating it

`release.pub` lists the current key first, then older keys still trusted. To rotate:

1. Make a new key as above.
2. Put its public key **first** in `release.pub`, keeping the old one below it, and replace the
   secret's value with the new secret key.
3. Release. That release signs its bundled Plugins with the new key, and still trusts packages the
   old key signed, so Plugins keefast published with it keep loading. An installed keefast Plugin
   updates from a package signed by either key.
4. Once nothing signed by the old key needs to load, remove its line in a later release.

If the secret key **leaks**, remove its line from `release.pub` at once, rotate, and release:
keefast versions from then on stop trusting what it signed, including packages installed from a
file, which then don't run, and Settings says why. Versions already released keep trusting it
until users update.
