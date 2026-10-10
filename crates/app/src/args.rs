//! The command line: `keefast [VAULT_FILE] [--key-file KEY_FILE]`, or
//! `keefast --unregister-browser-hosts`, which the Windows installer runs when it uninstalls.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Args {
    pub vault_file: Option<PathBuf>,
    pub key_file: Option<PathBuf>,
    pub unregister_browser_hosts: bool,
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
            } else if arg == "--unregister-browser-hosts" {
                parsed.unregister_browser_hosts = true;
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
    fn the_installer_can_ask_to_unregister_the_native_host() {
        let args = parse(&["--unregister-browser-hosts"]);

        assert!(args.unregister_browser_hosts);
        assert_eq!(args.vault_file, None);
    }

    #[test]
    fn opening_a_vault_file_does_not_ask_to_unregister_the_native_host() {
        let args = parse(&["personal.kdbx", "--key-file", "personal.keyx"]);

        assert!(!args.unregister_browser_hosts);
        assert!(args.vault_file.unwrap().ends_with("personal.kdbx"));
        assert!(args.key_file.unwrap().ends_with("personal.keyx"));
    }

    #[test]
    fn unknown_options_are_ignored() {
        let args = parse(&["--verbose", "work.kdbx"]);

        assert!(args.vault_file.unwrap().ends_with("work.kdbx"));
    }
}
