# Installing keefast

## Linux

Each release has an AppImage, a tarball and the Companion extension as a zip. keefast runs on
Wayland and X11, and draws its window with WebKitGTK 4.1.

- **AppImage**: `chmod +x keefast-*.AppImage` and run it.
- **Tarball**: unpack it and run `bin/keefast`, or install it for your user with
  `tar -xzf keefast-*-linux-*.tar.gz --strip-components=1 -C ~/.local`. It uses the system's
  WebKitGTK 4.1 (`libwebkit2gtk-4.1-0` on Debian and Ubuntu, `webkit2gtk4.1` on Fedora), which
  the AppImage brings along.
- **Nix**: `nix run github:arthur-er/keefast`, or add the flake to your NixOS configuration:

  ```nix
  # flake.nix
  inputs.keefast.url = "github:arthur-er/keefast";
  # configuration.nix
  environment.systemPackages = [ inputs.keefast.packages.${pkgs.system}.default ];
  ```

  The flake also has an overlay (`overlays.default`) that adds `pkgs.keefast`.

Browser integration works from every one of them: keefast registers the `keefast-native-host`
it shipped with. The AppImage is mounted somewhere new each run, so it copies its host to
`~/.local/share/keefast/` and registers that copy, refreshed each time it starts with browser
integration on. It copies `keefast-agent`, the command Agents run, to
`~/.local/share/keefast/keefast-agent` in the same way, each time it starts.

Releases are built by `.github/workflows/release.yml` when a push to main brings a new version
(see [Updates](#updates)): `scripts/package-linux.sh` makes the tarball and AppImage from a release build, and
`scripts/smoke-linux.sh` checks that keefast starts on X11 and Wayland.

## Windows

Each release has an MSI, `keefast-<version>-windows-x86_64.msi`. It installs keefast for your
user, without asking for administrator rights, in `%LOCALAPPDATA%\Programs\keefast`, with a
Start menu shortcut. Installing a newer MSI upgrades keefast in place. keefast draws its window
with the Microsoft Edge WebView2 Runtime, which Windows 11 and up-to-date Windows 10 have; the
MSI says so if it is missing.

Browser integration registers the `keefast-native-host` installed next to `keefast.exe`.
`keefast-agent.exe`, the command Agents run, is installed there too, and the MSI adds that
folder to your user's `PATH`.
Uninstalling keefast (Settings > Apps) removes those registrations, and only keefast's; your
config and Vault files stay, so a reinstalled keefast registers the host again when it starts
if browser integration was on.

Releases are built by `.github/workflows/release.yml` when a push to main brings a new version:
`scripts/package-windows.ps1` makes the MSI from a release build with WiX
(`packaging/windows/keefast.wxs`), and `scripts/smoke-windows.ps1` checks that it installs, and
that uninstalling removes keefast and its browser registrations but leaves another program's
alone. Pull requests that change the installer run that workflow too.

## Updates

keefast asks once whether to check for updates. Agreed, it asks GitHub for its latest release
when it starts and once a day after; General Settings > Updates turns that off, and has Check now
either way. Nothing but the request is sent, though GitHub sees your IP address. An update is
only ever installed when you click Update and restart: the AppImage replaces itself and the
Windows install runs the new MSI, then keefast restarts. Installs that don't update themselves
(the tarball, Nix, a build of your own) link to the release instead. keefast won't update while a
Vault has unsaved changes.

Updates are signed: keefast installs only a package signed with its update key, whose public key
is in `crates/app/tauri.conf.json` (ADR 0014). A release signs the AppImage and the MSI with it
(`scripts/sign-update.sh`), and attaches `latest.json` (`scripts/update-manifest.sh`) to the
release. A release is published, so offered to installed keefasts, as soon as a push to main
brings a version in `Cargo.toml` that has no `v<version>` tag yet: releasing is merging a version
bump, and CI tags the commit. The release workflows
need the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`,
from `cargo tauri signer generate -w ~/.tauri/keefast-updater.key`; keep a copy of the key
offline, as without it no installed keefast can be updated again. To try an update without a
release, run keefast with `KEEFAST_UPDATE_URL` pointing at a `latest.json` of your own, signed
with a key whose public key keefast was built with; a debug build accepts plain HTTP.
