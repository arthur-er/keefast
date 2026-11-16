# keefast can keep running in the Tray, runs once per user, and can start on boot

The Companion extension only gets Entries from a running keefast (ADR 0001), and so do Agents on the same machine (ADR 0012). Until now, closing the window quit keefast, so the browser stopped filling the moment the user tidied their taskbar. Three opt-in or invisible changes let keefast stay running without staying in the way.

## Close to tray

- **A setting, off by default.** "Close to tray" lives in the app config (`close_to_tray`), in General Settings under "Window & startup". Off, closing quits as before.
- **On, closing hides the window** and keefast keeps running behind a Tray icon. The icon exists only while the setting is on, added and removed as it changes, with no restart. A click shows the window (`bring_to_front`, where the platform reports clicks; Linux AppIndicators open the menu instead). Its menu: Show keefast, Lock all Vaults, and Quit. No notification says keefast is still running: the user turned the setting on.
- **Without a Tray, closing minimizes.** keefast must never be running with no way back to its window. On Linux it looks for `org.kde.StatusNotifierWatcher` on the session bus (KDE, most other desktops, GNOME only with an AppIndicator extension), and for the libayatana-appindicator library Tauri draws Tray icons with, at startup and whenever the setting is turned on. Without either, closing minimizes and the setting says "No system tray found; closing will minimize". Windows always has a notification area. What closing does is a pure function of the setting and whether a Tray was found (`tray::close_action`), so it is unit-tested.
- **Quitting is explicit.** The Tray's Quit, a `quit` shortcut (Ctrl Q, rebindable, working with the setting off as well) and a "Quit keefast" command palette entry all exit the same way closing used to: what keefast copied is cleared on the way out.
- **Packaging.** libayatana-appindicator is loaded at runtime (dlopen), not linked, so packagers must ship it on purpose: the AppImage carries it in its `usr/lib`, and the Nix wrapper puts it on `LD_LIBRARY_PATH`.

## Single instance

keefast always runs once per user, with `tauri-plugin-single-instance` (a D-Bus name on Linux, a named mutex on Windows), called from Rust only (ADR 0008). A second launch hands its command line to the running keefast and exits. The running one comes to the front, and offers a Vault file the launch named to unlock, unless that Vault is unlocked already. Two keefasts would fight over the native host's socket or pipe and over the app config, and a hidden one would be found only by launching keefast again. Without a session bus, the plugin can't find another instance and keefast starts as before.

## Start on boot

An opt-in "Start on boot" setting (#166), second in "Window & startup", registers `keefast --hidden` to run when the user logs in: an XDG autostart entry on Linux, HKCU's `Run` key on Windows. The OS registration is the truth, read back each time, so an entry removed by hand shows as off. A `--hidden` launch starts with only the Tray icon when Close to tray is on and a Tray was found, and minimized otherwise. It never unlocks anything. `keefast --unregister` (which replaces `--unregister-browser-hosts`, still accepted) removes the autostart entry with the native host registrations, and the Windows MSI runs it on uninstall.

## Vaults stay unlocked while hidden

Hiding is not locking. The point of keeping keefast running is that the Companion extension can fill from unlocked Vaults, and locking on hide would make Close to tray a slower quit. The protections for an unattended machine (ADR 0004) still apply: auto-lock on idle counts only input in keefast's window, so a hidden keefast locks once the idle wait passes; locking with the screen and on suspend don't depend on the window; and the Tray menu has Lock all Vaults. A user who wants Vaults locked when the window closes can leave the setting off, or lock first. Starting on boot never unlocks, so a login gives an attacker nothing an unlocked desktop wouldn't.
