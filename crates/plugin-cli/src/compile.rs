//! `keefast-plugin build`: a Plugin's crate built for `wasm32-unknown-unknown`, then made into the
//! WebAssembly component keefast runs, from the metadata the SDK's bindings leave in it.
//!
//! The build is reproducible: paths on the building machine are remapped out of the binary, so
//! the same source, lockfile and toolchain give the same bytes anywhere. Except for a crate the
//! Plugin depends on by a path outside its Cargo workspace: Cargo identifies that one by its
//! absolute path, which reaches its symbol names and with them the layout of the component's
//! data, so the build warns of it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET: &str = "wasm32-unknown-unknown";

/// The component of the Plugin crate in `dir` (canonical), built in release mode.
pub fn component(dir: &Path) -> Result<Vec<u8>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let workspace = workspace_root(&cargo, dir)?;
    let locked = workspace.join("Cargo.lock").exists();
    let crate_ = Crate::read(&cargo, dir, locked)?;
    for (name, path) in &crate_.path_dependencies {
        if !path.starts_with(&workspace) {
            eprintln!(
                "warning: {name} ({}) is outside the Plugin's Cargo workspace ({}), so the \
                 component's bytes depend on where it is checked out. Make it a workspace member, \
                 or depend on it from git or crates.io",
                path.display(),
                workspace.display()
            );
        }
    }
    let target_dir = dir.join("target");

    let mut build = Command::new(&cargo);
    build
        .current_dir(dir)
        .args(["build", "--release", "--target", TARGET, "--lib"])
        .env("CARGO_TARGET_DIR", &target_dir)
        .env(
            "CARGO_ENCODED_RUSTFLAGS",
            rustflags(dir, &crate_.path_dependencies),
        )
        .env_remove("RUSTFLAGS");
    if locked {
        build.arg("--locked");
    }
    let status = build
        .status()
        .map_err(|error| format!("running cargo: {error}"))?;
    if !status.success() {
        return Err(format!(
            "cargo build failed. A Plugin is built for {TARGET}: `rustup target add {TARGET}` \
             if your toolchain hasn't it"
        ));
    }

    let core = target_dir
        .join(TARGET)
        .join("release")
        .join(format!("{}.wasm", crate_.lib));
    let module = std::fs::read(&core).map_err(|error| format!("{}: {error}", core.display()))?;
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .and_then(|encoder| encoder.encode())
        .map_err(|error| {
            format!(
                "{} isn't a Plugin's module: {error:#}. Does the crate call \
                 keefast_plugin_sdk::export!?",
                core.display()
            )
        })
}

/// The root directory of the Cargo workspace the Plugin's crate in `dir` belongs to: `dir` itself
/// when the crate is a workspace of its own.
fn workspace_root(cargo: &OsString, dir: &Path) -> Result<PathBuf, String> {
    let output = Command::new(cargo)
        .current_dir(dir)
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .output()
        .map_err(|error| format!("running cargo: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo locate-project failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let manifest = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    Ok(manifest.parent().unwrap_or(dir).to_path_buf())
}

/// What `cargo metadata` says of the Plugin's crate.
struct Crate {
    /// The file name cargo gives its `cdylib`.
    lib: String,
    /// The crates it depends on by path, other than itself: their names and directories.
    path_dependencies: Vec<(String, PathBuf)>,
}

impl Crate {
    fn read(cargo: &OsString, dir: &Path, locked: bool) -> Result<Self, String> {
        let mut command = Command::new(cargo);
        command
            .current_dir(dir)
            .args(["metadata", "--format-version", "1"]);
        if locked {
            command.arg("--locked");
        }
        let output = command
            .output()
            .map_err(|error| format!("running cargo: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo metadata failed in {}: {}",
                dir.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let metadata: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
        let manifest = dir.join("Cargo.toml");
        let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
        let manifest_of =
            |package: &serde_json::Value| package["manifest_path"].as_str().map(PathBuf::from);
        let lib = packages
            .iter()
            .filter(|package| manifest_of(package).as_deref() == Some(manifest.as_path()))
            .flat_map(|package| package["targets"].as_array().into_iter().flatten())
            .find(|target| {
                target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "cdylib"))
            })
            .and_then(|target| target["name"].as_str())
            .map(|name| name.replace('-', "_"))
            .ok_or_else(|| {
                format!(
                    "{} has no cdylib: a Plugin's crate needs crate-type = [\"cdylib\"] under \
                     [lib]",
                    manifest.display()
                )
            })?;
        // Crates from crates.io and git have a `source`; those by path have none.
        let path_dependencies = packages
            .iter()
            .filter(|package| package["source"].is_null())
            .filter_map(|package| {
                let path = manifest_of(package)?;
                let name = package["name"].as_str()?.to_owned();
                let dir = path.parent()?.to_path_buf();
                (path != manifest).then_some((name, dir))
            })
            .collect();
        Ok(Self {
            lib,
            path_dependencies,
        })
    }
}

/// The flags rustc gets: the user's `RUSTFLAGS`, then remappings of the paths that would
/// otherwise end up in the binary: the Plugin's directory, its path dependencies', and Cargo's
/// home, where dependencies from crates.io and git are unpacked.
fn rustflags(dir: &Path, path_dependencies: &[(String, PathBuf)]) -> OsString {
    let mut flags: Vec<OsString> = std::env::var("RUSTFLAGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(OsString::from)
        .collect();
    let mut remap = |from: &Path, to: &str| {
        let mut flag = OsString::from("--remap-path-prefix=");
        flag.push(from);
        flag.push("=");
        flag.push(to);
        flags.push(flag);
    };
    remap(dir, "/plugin");
    for (name, path) in path_dependencies {
        remap(path, &format!("/path/{name}"));
    }
    remap(&cargo_home(), "/cargo");
    let mut joined = OsString::new();
    for (index, flag) in flags.iter().enumerate() {
        if index > 0 {
            joined.push("\u{1f}");
        }
        joined.push(flag);
    }
    joined
}

fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".cargo")))
        .unwrap_or_default()
}
