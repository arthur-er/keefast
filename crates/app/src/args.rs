//! The command line: `keefast [VAULT_FILE] [--key-file KEY_FILE] [--hidden]`, or
//! `keefast --unregister`, which the Windows installer runs when it uninstalls.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::autostart::HIDDEN_ARG;

/// What installers before `--unregister` run when they uninstall: kept so they still remove
/// everything keefast registered.
const UNREGISTER_BROWSER_HOSTS: &str = "--unregister-browser-hosts";

#[derive(Debug, Default)]
pub struct Args {
    pub vault_file: Option<PathBuf>,
    pub key_file: Option<PathBuf>,
    /// Start without showing the window, as Start on boot does.
    pub hidden: bool,
    /// Remove what keefast registered with the OS (the native host's registrations with the
    /// browsers, and Start on boot) and exit, without a window.
    pub unregister: bool,
}

impl Args {
    pub fn parse(mut args: impl Iterator<Item = OsString>) -> Self {
        // Recent Vault files are remembered by full path, so relative paths are made absolute.
        let absolute =
            |path: OsString| std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(path));
        let mut parsed = Args::default();
        while let Some(arg) = args.next() {
            if arg == "--key-file" {
                parsed.key_file = args.next().map(absolute);
            } else if arg == HIDDEN_ARG {
                parsed.hidden = true;
            } else if arg == "--unregister" || arg == UNREGISTER_BROWSER_HOSTS {
                parsed.unregister = true;
            } else if arg.to_string_lossy().starts_with("--") {
                eprintln!("keefast: ignoring unknown option {}", arg.to_string_lossy());
            } else if parsed.vault_file.is_none() {
                parsed.vault_file = Some(absolute(arg));
            }
        }
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn the_installer_can_ask_to_unregister() {
        let args = parse(&["--unregister"]);

        assert!(args.unregister);
        assert!(!args.hidden);
        assert_eq!(args.vault_file, None);
    }

    #[test]
    fn installers_from_before_unregister_still_unregister_everything() {
        let args = parse(&["--unregister-browser-hosts"]);

        assert!(args.unregister);
        assert_eq!(args.vault_file, None);
    }

    #[test]
    fn start_on_boot_starts_hidden() {
        let args = parse(&["--hidden"]);

        assert!(args.hidden);
        assert!(!args.unregister);
        assert_eq!(args.vault_file, None);
    }

    #[test]
    fn a_vault_file_can_be_opened_hidden() {
        let args = parse(&["--hidden", "personal.kdbx"]);

        assert!(args.hidden);
        assert!(args.vault_file.unwrap().ends_with("personal.kdbx"));
    }

    #[test]
    fn opening_a_vault_file_neither_hides_nor_unregisters() {
        let args = parse(&["personal.kdbx", "--key-file", "personal.keyx"]);

        assert!(!args.unregister);
        assert!(!args.hidden);
        assert!(args.vault_file.unwrap().ends_with("personal.kdbx"));
        assert!(args.key_file.unwrap().ends_with("personal.keyx"));
    }

    #[test]
    fn unknown_options_are_ignored() {
        let args = parse(&["--verbose", "work.kdbx"]);

        assert!(args.vault_file.unwrap().ends_with("work.kdbx"));
    }
}
