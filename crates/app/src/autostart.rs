//! Start on boot: keefast registered with the OS to start, with `--hidden`, when the user logs in.
//!
//! The OS registration is the truth: no setting remembers it, so an entry the user removed by
//! hand reads as off. On Linux it is an XDG autostart entry, `$XDG_CONFIG_HOME/autostart/
//! keefast.desktop`; on Windows, a value under HKCU's `Run` key, which `tauri-plugin-autostart`
//! writes.
//!
//! Linux writes its own entry because the plugin registers `current_exe()` and can't be told
//! otherwise. In the Nix package that is the unwrapped `/nix/store/<hash>-keefast-…/bin/
//! .keefast-wrapped`: missing what its wrapper gives WebKitGTK, and gone after an upgrade and a
//! garbage collection. The plugin also writes the path into `Exec` unquoted. See [`exec_path`]
//! for the path keefast registers instead.

#[cfg(target_os = "linux")]
use std::ffi::{OsStr, OsString};
use std::io;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Runtime};

/// What a boot launch passes, to start without showing the window.
pub const HIDDEN_ARG: &str = "--hidden";

/// The name keefast registers under: the autostart entry's file name on Linux, the `Run`
/// value's on Windows.
const ENTRY_NAME: &str = "keefast";

/// How a `--hidden` launch starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenLaunch {
    /// With only the tray icon: the window stays hidden until the user asks for it.
    TrayOnly,
    /// With the window minimized to the taskbar.
    Minimized,
}

/// How a `--hidden` launch starts: in the tray when closing goes to the tray and there is one,
/// else minimized, so keefast is never running with no way back to its window. It never unlocks
/// anything either way.
pub fn hidden_launch(close_to_tray: bool, tray_available: bool) -> HiddenLaunch {
    if close_to_tray && tray_available {
        HiddenLaunch::TrayOnly
    } else {
        HiddenLaunch::Minimized
    }
}

/// The plugin that registers keefast on Windows, with `--hidden`.
#[cfg(not(target_os = "linux"))]
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_autostart::Builder::new()
        .app_name(ENTRY_NAME)
        .arg(HIDDEN_ARG)
        .build()
}

/// Whether keefast is registered to start on boot, as the OS has it now.
pub fn is_enabled<R: Runtime>(app: &AppHandle<R>) -> io::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        linux::is_enabled()
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().map_err(io::Error::other)
    }
}

/// Registers keefast to start on boot, or removes its registration.
pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        if enabled {
            linux::enable(&exec_path(&Invocation::current()))
        } else {
            linux::disable()
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = app.autolaunch();
        let result = if enabled {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        result.map_err(io::Error::other)
    }
}

/// Removes keefast's registration, without a window: for `--unregister`, which the Windows
/// installer runs when it uninstalls. Nothing registered is not a failure.
pub fn unregister() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::disable()
    }
    #[cfg(windows)]
    {
        // What the plugin registered with; only the name matters for removing it.
        let exe = std::env::current_exe()?;
        auto_launch::AutoLaunchBuilder::new()
            .set_app_name(ENTRY_NAME)
            .set_app_path(&exe.to_string_lossy())
            .set_args(&[HIDDEN_ARG])
            .build()
            .and_then(|entry| entry.disable())
            .map_err(io::Error::other)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Ok(())
    }
}

/// How this keefast was started, as far as where its program is goes.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Default)]
struct Invocation {
    /// `$APPIMAGE`: the AppImage file, when keefast runs from one.
    appimage: Option<OsString>,
    /// `$APPDIR`: where the AppImage is mounted while it runs.
    appdir: Option<OsString>,
    /// The program running, links resolved.
    exe: Option<PathBuf>,
    /// The program's name as it was started (`argv[0]`): a path, or a name looked up on `PATH`.
    argv0: Option<OsString>,
    /// `$PATH`.
    path: Option<OsString>,
}

#[cfg(target_os = "linux")]
impl Invocation {
    fn current() -> Self {
        Self {
            appimage: std::env::var_os("APPIMAGE"),
            appdir: std::env::var_os("APPDIR"),
            exe: std::env::current_exe().ok(),
            argv0: std::env::args_os().next(),
            path: std::env::var_os("PATH"),
        }
    }
}

/// Where the Nix store is.
#[cfg(target_os = "linux")]
const NIX_STORE: &str = "/nix/store";

/// The program the autostart entry starts, chosen to outlive this run:
///
/// 1. An AppImage registers the AppImage file (`$APPIMAGE`), not where it is mounted while it
///    runs, which is a new temporary directory each time. `$APPIMAGE` counts only when keefast
///    runs from inside `$APPDIR`, so one inherited from another AppImage doesn't.
/// 2. A program outside the Nix store registers itself.
/// 3. In the Nix store, the store path changes with every build and is garbage collected after
///    an upgrade, so keefast registers how it was found instead: a profile's `bin/keefast`,
///    such as `/run/current-system/sw/bin/keefast` or `~/.nix-profile/bin/keefast`, which
///    follows upgrades. That is `argv[0]` (as typed, or looked up on `PATH`), else `keefast` on
///    `PATH`, when it lies outside the store and leads to this same package's `bin`.
/// 4. Failing that, the package's own `bin/keefast` in the store (its wrapper, not the
///    unwrapped program running), which works until it is garbage collected.
#[cfg(target_os = "linux")]
fn exec_path(invocation: &Invocation) -> PathBuf {
    exec_path_with_store(invocation, Path::new(NIX_STORE))
}

#[cfg(target_os = "linux")]
fn exec_path_with_store(invocation: &Invocation, store: &Path) -> PathBuf {
    let exe = invocation
        .exe
        .clone()
        .unwrap_or_else(|| PathBuf::from("keefast"));
    if let (Some(appimage), Some(appdir)) = (&invocation.appimage, &invocation.appdir) {
        let appimage = PathBuf::from(appimage);
        if appimage.is_absolute() && appimage.is_file() && exe.starts_with(appdir) {
            return appimage;
        }
    }
    if !exe.starts_with(store) {
        return exe;
    }
    let Some(package_bin) = exe.parent() else {
        return exe;
    };
    let launched = invocation.argv0.as_deref().and_then(|argv0| {
        if Path::new(argv0).components().count() > 1 {
            std::path::absolute(argv0).ok()
        } else {
            on_path(argv0, invocation.path.as_deref())
        }
    });
    let found = on_path(OsStr::new("keefast"), invocation.path.as_deref());
    let stable = [launched, found].into_iter().flatten().find(|candidate| {
        !candidate.starts_with(store)
            && candidate
                .canonicalize()
                .is_ok_and(|real| real.parent() == Some(package_bin))
    });
    if let Some(stable) = stable {
        return stable;
    }
    // wrapGApp hides the program as `.keefast-wrapped` behind a `keefast` script.
    let wrapper = package_bin.join("keefast");
    if wrapper.is_file() { wrapper } else { exe }
}

/// The first file called `name` in a `PATH`'s directories.
#[cfg(target_os = "linux")]
fn on_path(name: &OsStr, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The XDG autostart entry.
#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use super::{ENTRY_NAME, HIDDEN_ARG};

    /// `$XDG_CONFIG_HOME/autostart/keefast.desktop`, where desktops look for programs to start
    /// when the user logs in.
    fn entry_path() -> io::Result<PathBuf> {
        dirs::config_dir()
            .map(|config| {
                config
                    .join("autostart")
                    .join(format!("{ENTRY_NAME}.desktop"))
            })
            .ok_or_else(|| io::Error::other("no config directory for the current user"))
    }

    pub fn is_enabled() -> io::Result<bool> {
        match fs::read_to_string(entry_path()?) {
            Ok(entry) => Ok(!turned_off(&entry)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn enable(exec: &Path) -> io::Result<()> {
        let path = entry_path()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, desktop_entry(exec))
    }

    pub fn disable() -> io::Result<()> {
        match fs::remove_file(entry_path()?) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Whether an entry is there but switched off, as GNOME's and KDE's startup settings do
    /// rather than deleting it.
    pub(super) fn turned_off(entry: &str) -> bool {
        entry.lines().map(str::trim).any(|line| {
            line.eq_ignore_ascii_case("Hidden=true")
                || line.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
        })
    }

    pub(super) fn desktop_entry(exec: &Path) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=KeeFast\n\
             Comment=Starts keefast in the background when you log in\n\
             Exec={} {HIDDEN_ARG}\n\
             Icon=keefast\n\
             Terminal=false\n\
             StartupNotify=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exec_argument(&exec.to_string_lossy())
        )
    }

    /// `argument` quoted for an `Exec` line, as the Desktop Entry Specification has it: inside
    /// double quotes, `"`, `` ` ``, `$` and `\` are escaped with a backslash, then every
    /// backslash is doubled as in any string value, and `%` is written `%%`.
    pub(super) fn exec_argument(argument: &str) -> String {
        let mut quoted = String::from("\"");
        for c in argument.chars() {
            match c {
                '"' | '`' | '$' => {
                    quoted.push_str("\\\\");
                    quoted.push(c);
                }
                '\\' => quoted.push_str("\\\\\\\\"),
                '%' => quoted.push_str("%%"),
                _ => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_launch_goes_to_the_tray_only_when_closing_does_and_there_is_one() {
        assert_eq!(hidden_launch(true, true), HiddenLaunch::TrayOnly);
        assert_eq!(hidden_launch(true, false), HiddenLaunch::Minimized);
        assert_eq!(hidden_launch(false, true), HiddenLaunch::Minimized);
        assert_eq!(hidden_launch(false, false), HiddenLaunch::Minimized);
    }

    #[cfg(target_os = "linux")]
    mod linux_entry {
        use std::os::unix::fs::symlink;
        use std::path::Path;

        use super::super::linux::{desktop_entry, exec_argument, turned_off};
        use super::super::*;

        fn file(path: &Path) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }

        #[test]
        fn the_entry_starts_keefast_hidden() {
            let entry = desktop_entry(Path::new("/home/me/Apps/KeeFast.AppImage"));

            assert!(entry.starts_with("[Desktop Entry]\n"));
            assert!(entry.contains("\nExec=\"/home/me/Apps/KeeFast.AppImage\" --hidden\n"));
            assert!(!turned_off(&entry));
        }

        #[test]
        fn exec_paths_are_quoted_as_desktop_entries_need() {
            assert_eq!(
                exec_argument("/home/me/My Apps/k"),
                "\"/home/me/My Apps/k\""
            );
            assert_eq!(exec_argument("/a$b\"c`d"), r#""/a\\$b\\"c\\`d""#);
            assert_eq!(exec_argument(r"/a\b"), r#""/a\\\\b""#);
            assert_eq!(exec_argument("/100%"), "\"/100%%\"");
        }

        #[test]
        fn an_entry_the_desktop_switched_off_reads_as_off() {
            assert!(turned_off("[Desktop Entry]\nHidden=true\n"));
            assert!(turned_off(
                "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n"
            ));
            assert!(!turned_off(
                "[Desktop Entry]\nX-GNOME-Autostart-enabled=true\n"
            ));
        }

        #[test]
        fn an_appimage_registers_the_appimage_file_not_its_mount() {
            let dir = tempfile::tempdir().unwrap();
            let appimage = dir.path().join("KeeFast.AppImage");
            file(&appimage);
            let invocation = Invocation {
                appimage: Some(appimage.clone().into()),
                appdir: Some("/tmp/.mount_KeeFasXYZ".into()),
                exe: Some("/tmp/.mount_KeeFasXYZ/usr/bin/keefast".into()),
                ..Invocation::default()
            };

            assert_eq!(exec_path(&invocation), appimage);
        }

        #[test]
        fn an_appimage_variable_from_elsewhere_is_ignored() {
            let dir = tempfile::tempdir().unwrap();
            let appimage = dir.path().join("Other.AppImage");
            file(&appimage);
            let invocation = Invocation {
                appimage: Some(appimage.into()),
                appdir: Some("/tmp/.mount_OtherXYZ".into()),
                exe: Some("/usr/bin/keefast".into()),
                ..Invocation::default()
            };

            assert_eq!(exec_path(&invocation), Path::new("/usr/bin/keefast"));
        }

        /// A Nix store with keefast's package, wrapped as wrapGApp does, and a profile linking
        /// to it.
        struct Nix {
            _dir: tempfile::TempDir,
            store: PathBuf,
            wrapped: PathBuf,
            wrapper: PathBuf,
            profile_bin: PathBuf,
        }

        fn nix() -> Nix {
            let dir = tempfile::tempdir().unwrap();
            let store = dir.path().join("nix/store");
            let package_bin = store.join("abc123-keefast-0.1.0/bin");
            let wrapped = package_bin.join(".keefast-wrapped");
            let wrapper = package_bin.join("keefast");
            file(&wrapped);
            file(&wrapper);
            let profile_bin = dir.path().join("profile/bin");
            std::fs::create_dir_all(&profile_bin).unwrap();
            symlink(&wrapper, profile_bin.join("keefast")).unwrap();
            Nix {
                _dir: dir,
                store,
                wrapped,
                wrapper,
                profile_bin,
            }
        }

        #[test]
        fn nix_registers_the_profile_keefast_was_started_from() {
            let nix = nix();
            let invocation = Invocation {
                exe: Some(nix.wrapped.clone()),
                argv0: Some(nix.profile_bin.join("keefast").into()),
                ..Invocation::default()
            };

            assert_eq!(
                exec_path_with_store(&invocation, &nix.store),
                nix.profile_bin.join("keefast")
            );
        }

        #[test]
        fn nix_looks_keefast_up_on_path_when_started_by_name() {
            let nix = nix();
            let invocation = Invocation {
                exe: Some(nix.wrapped.clone()),
                argv0: Some("keefast".into()),
                path: Some(
                    std::env::join_paths(["/nonexistent".as_ref(), nix.profile_bin.as_path()])
                        .unwrap(),
                ),
                ..Invocation::default()
            };

            assert_eq!(
                exec_path_with_store(&invocation, &nix.store),
                nix.profile_bin.join("keefast")
            );
        }

        #[test]
        fn nix_falls_back_to_the_packages_wrapper_without_a_profile_leading_to_it() {
            let nix = nix();
            let other = nix.store.join("def456-keefast-0.2.0/bin/keefast");
            file(&other);
            let elsewhere = nix._dir.path().join("elsewhere/bin");
            std::fs::create_dir_all(&elsewhere).unwrap();
            symlink(&other, elsewhere.join("keefast")).unwrap();
            let invocation = Invocation {
                exe: Some(nix.wrapped.clone()),
                // `nix run`: started from the store, and another version on PATH.
                argv0: Some(nix.wrapper.clone().into()),
                path: Some(elsewhere.into()),
                ..Invocation::default()
            };

            assert_eq!(exec_path_with_store(&invocation, &nix.store), nix.wrapper);
        }

        #[test]
        fn a_program_outside_the_store_registers_itself() {
            let invocation = Invocation {
                exe: Some("/home/me/keefast/target/debug/keefast".into()),
                ..Invocation::default()
            };

            assert_eq!(
                exec_path(&invocation),
                Path::new("/home/me/keefast/target/debug/keefast")
            );
        }
    }
}
