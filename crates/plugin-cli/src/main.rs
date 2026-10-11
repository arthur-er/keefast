//! `keefast-plugin`: builds a keefast Plugin and packs it into a signed `.kfplugin`.
//!
//! ```text
//! keefast-plugin keygen <secret key file>
//! keefast-plugin build [<plugin dir>] [-o <component.wasm>]
//! keefast-plugin pack (--key <secret key file> | --key-env <VAR>) [--manifest plugin.json]
//!                     [--component plugin.wasm] [--icon icon.png] -o <out.kfplugin>
//! keefast-plugin verify <package.kfplugin> [--publisher-key <base64>]
//! ```
//!
//! docs/plugins/README.md is the guide.

mod compile;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use keefast_plugin_package::{
    PublisherKey, SigningKey, parse_secret_key, secret_key_to_base64, with_publisher_key,
};
use keefast_plugins::{Package, PluginHost, is_release_key};

const USAGE: &str = "\
keefast-plugin: build a keefast Plugin and pack it into a signed .kfplugin

usage:
  keefast-plugin keygen <secret key file>
      Makes a new ed25519 key to sign your Plugin's packages with, writes its secret half to the
      file, and prints its public half for plugin.json's publisherKey. Keep the file secret and
      safe: every update of your Plugin must be signed with it.

  keefast-plugin build [<plugin dir>] [-o <component.wasm>]
      Builds the Plugin's crate for wasm32-unknown-unknown and makes the WebAssembly component
      keefast runs. Writes <plugin dir>/target/plugin.wasm unless -o says otherwise.

  keefast-plugin pack (--key <secret key file> | --key-env <VAR>) [--manifest plugin.json]
                      [--component <plugin dir>/target/plugin.wasm] [--icon icon.png]
                      -o <out.kfplugin>
      Signs the manifest, component and icon into a package, after checking keefast would
      install it. Adds publisherKey to the packaged manifest if plugin.json has none.

  keefast-plugin verify <package.kfplugin> [--publisher-key <base64>]
      Checks a package as keefast does when installing it, and says what it holds.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") => keygen(&args[1..]),
        Some("build") => build(&args[1..]),
        Some("pack") => pack(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("help" | "--help" | "-h") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A command's arguments: options with a value, and the rest in order.
struct Args {
    options: Vec<(String, String)>,
    positional: Vec<String>,
}

impl Args {
    fn parse(args: &[String], known: &[&str]) -> Result<Self, String> {
        let mut options = Vec::new();
        let mut positional = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if arg.starts_with('-') && arg.len() > 1 {
                if !known.contains(&arg.as_str()) {
                    return Err(format!("unknown option {arg}; see keefast-plugin --help"));
                }
                let value = args.next().ok_or(format!("{arg} needs a value"))?;
                options.push((arg.clone(), value.clone()));
            } else {
                positional.push(arg.clone());
            }
        }
        Ok(Self {
            options,
            positional,
        })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(option, _)| option == name)
            .map(|(_, value)| value.as_str())
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn keygen(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args, &[])?;
    let [path] = args.positional.as_slice() else {
        return Err("usage: keefast-plugin keygen <secret key file>".into());
    };
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| error.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    write_secret(Path::new(path), secret_key_to_base64(&key).as_bytes())?;
    let public = PublisherKey::of(&key);
    println!("Wrote the secret key to {path}. Keep it secret, and back it up:");
    println!("every update of your Plugin must be signed with it.");
    println!();
    println!("publisherKey: {}", public.to_base64());
    println!("fingerprint:  {}", public.fingerprint());
    Ok(())
}

/// Writes a new file only its owner can read, refusing to replace one.
fn write_secret(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!("{} exists; keygen never replaces a key", path.display())
        } else {
            format!("{}: {error}", path.display())
        }
    })?;
    file.write_all(bytes)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn build(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args, &["-o"])?;
    let dir = match args.positional.as_slice() {
        [] => PathBuf::from("."),
        [dir] => PathBuf::from(dir),
        _ => return Err("usage: keefast-plugin build [<plugin dir>] [-o <component.wasm>]".into()),
    };
    let dir = dir
        .canonicalize()
        .map_err(|error| format!("{}: {error}", dir.display()))?;
    let out = args
        .get("-o")
        .map_or_else(|| dir.join("target").join("plugin.wasm"), PathBuf::from);
    let component = compile::component(&dir)?;
    if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&out, &component).map_err(|error| format!("{}: {error}", out.display()))?;
    println!("built {} ({} bytes)", out.display(), component.len());
    Ok(())
}

fn pack(args: &[String]) -> Result<(), String> {
    let args = Args::parse(
        args,
        &[
            "--key",
            "--key-env",
            "--manifest",
            "--component",
            "--icon",
            "-o",
        ],
    )?;
    if !args.positional.is_empty() {
        return Err(format!(
            "unexpected {}; see keefast-plugin --help",
            args.positional[0]
        ));
    }
    let key = match (args.get("--key"), args.get("--key-env")) {
        (Some(path), None) => {
            let text = String::from_utf8(read(Path::new(path))?)
                .map_err(|_| format!("{path} isn't a key made by keefast-plugin keygen"))?;
            parse_secret_key(&text).map_err(|error| format!("{path}: {error}"))?
        }
        (None, Some(var)) => {
            let text = std::env::var(var).map_err(|_| format!("${var} isn't set"))?;
            parse_secret_key(&text).map_err(|error| format!("${var}: {error}"))?
        }
        _ => return Err("pack needs --key <secret key file> or --key-env <VAR>".into()),
    };
    let out = args.get("-o").ok_or("pack needs -o <out.kfplugin>")?;
    let manifest_path = PathBuf::from(args.get("--manifest").unwrap_or("plugin.json"));
    let component_path = args.get("--component").map_or_else(
        || {
            manifest_path
                .parent()
                .unwrap_or(Path::new("."))
                .join("target")
                .join("plugin.wasm")
        },
        PathBuf::from,
    );
    let manifest = String::from_utf8(read(&manifest_path)?)
        .map_err(|_| format!("{} isn't UTF-8 text", manifest_path.display()))?;
    let public = PublisherKey::of(&key);
    let named = manifest.contains("\"publisherKey\"");
    let manifest = with_publisher_key(&manifest, &public).map_err(|error| {
        format!(
            "{}: {error}. Sign with the key it names, or change publisherKey to {}",
            manifest_path.display(),
            public.to_base64()
        )
    })?;
    let component = read(&component_path)?;
    let icon = args.get("--icon").map(Path::new).map(read).transpose()?;

    let package = keefast_plugins::package::pack(&manifest, &component, icon.as_deref(), &key)
        .map_err(|error| error.to_string())?;
    let checked = check(&package)?;
    fs::write(out, &package).map_err(|error| format!("{out}: {error}"))?;
    println!(
        "packed {} {} into {out} ({} bytes)",
        checked.manifest.id,
        checked.manifest.version,
        package.len()
    );
    println!(
        "signed by {} ({})",
        public.to_base64(),
        public.fingerprint()
    );
    for line in shortcuts(&checked) {
        println!("{line}");
    }
    if !named {
        println!(
            "note: {} names no publisherKey, so the package names the signing key. Add \
             \"publisherKey\": \"{}\" to it, so packing with another key by mistake fails.",
            manifest_path.display(),
            public.to_base64()
        );
    }
    Ok(())
}

fn verify(args: &[String]) -> Result<(), String> {
    let args = Args::parse(args, &["--publisher-key"])?;
    let [path] = args.positional.as_slice() else {
        return Err(
            "usage: keefast-plugin verify <package.kfplugin> [--publisher-key <base64>]".into(),
        );
    };
    let package = check(&read(Path::new(path))?)?;
    if let Some(expected) = args.get("--publisher-key") {
        let expected = PublisherKey::parse(expected).map_err(|error| error.to_string())?;
        if package.key != expected {
            return Err(format!(
                "{path} is signed by {} ({}), not by the key given",
                package.key.to_base64(),
                package.key.fingerprint()
            ));
        }
    }
    let manifest = &package.manifest;
    println!("{path}: a Plugin package keefast can install");
    println!("  id           {}", manifest.id);
    println!("  name         {}", manifest.name);
    println!("  version      {}", manifest.version);
    println!(
        "  publisher    {}",
        manifest.publisher.as_deref().unwrap_or_default()
    );
    println!(
        "  signed by    {} ({})",
        package.key.to_base64(),
        package.key.fingerprint()
    );
    println!("  digest       {}", package.digest);
    let capabilities: Vec<String> = manifest
        .capabilities
        .iter()
        .map(|capability| {
            // As the manifest names it: `readProtected`, `copy`.
            serde_json::to_value(capability)
                .ok()
                .and_then(|name| name.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
        .collect();
    println!("  capabilities {}", list(&capabilities));
    let owned: Vec<String> = manifest.owned_names().map(str::to_owned).collect();
    println!("  owned fields {}", list(&owned));
    for line in shortcuts(&package) {
        println!("{line}");
    }
    Ok(())
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}

/// The shortcuts the package's commands would get beside keefast's own Plugins, and why any
/// would be refused: keefast refuses the shortcut, not the Plugin, so this only warns.
fn shortcuts(package: &Package) -> Vec<String> {
    let commands: Vec<_> = package
        .manifest
        .contributions
        .facets
        .iter()
        .flat_map(|facet| facet.commands.iter().map(move |command| (facet, command)))
        .filter(|(_, command)| command.shortcut.is_some())
        .collect();
    if commands.is_empty() {
        return Vec::new();
    }
    // Beside keefast's own Plugins, as when installed; alone if it can't load beside them.
    let mut host = PluginHost::bundled();
    if host
        .load(&package.manifest_json, &package.component)
        .is_err()
    {
        host = PluginHost::new();
        let _ = host.load(&package.manifest_json, &package.component);
    }
    let id = &package.manifest.id;
    commands
        .into_iter()
        .map(|(facet, command)| {
            let refused = host.refused_shortcuts().iter().find(|refused| {
                &refused.plugin == id && refused.facet == facet.id && refused.command == command.id
            });
            match refused {
                Some(refused) => format!(
                    "warning: \"{}\" won't have a shortcut: {}",
                    command.title, refused.reason
                ),
                None => format!(
                    "  shortcut     {}: {}",
                    command.title,
                    command.shortcut.as_deref().unwrap_or_default()
                ),
            }
        })
        .collect()
}

/// Checks a package as keefast does before installing it: its container and signature, its
/// manifest, that its id isn't keefast's, and that its component loads in keefast's sandbox.
fn check(bytes: &[u8]) -> Result<Package, String> {
    let package = Package::read(bytes).map_err(|error| error.to_string())?;
    if package.manifest.id.starts_with("keefast.") && !is_release_key(&package.key) {
        return Err(format!(
            "ids starting \"keefast.\" are keefast's own, so keefast refuses {}",
            package.manifest.id
        ));
    }
    PluginHost::new()
        .load(&package.manifest_json, &package.component)
        .map_err(|error| error.to_string())?;
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_take_a_value() {
        let args: Vec<String> = ["a", "-o", "out", "b"].map(String::from).to_vec();
        let parsed = Args::parse(&args, &["-o"]).unwrap();
        assert_eq!(parsed.get("-o"), Some("out"));
        assert_eq!(parsed.positional, ["a", "b"]);
        assert!(Args::parse(&args, &[]).is_err());
        assert!(Args::parse(&["-o".to_owned()], &["-o"]).is_err());
    }

    #[test]
    fn keygen_never_replaces_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("publisher.key");
        let arg = vec![path.display().to_string()];
        keygen(&arg).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        assert!(parse_secret_key(&first).is_ok());
        assert!(keygen(&arg).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), first);
    }
}
