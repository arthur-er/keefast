//! Packs the Plugins bundled with keefast into signed packages (`.kfplugin`) for keefast to
//! compile in, so they load through the same checks as any installed Plugin (ADR 0009).
//!
//! An official release builds with `KEEFAST_SIGNED_PLUGINS` naming a directory of packages the
//! release workflow signed beforehand with keefast's release key (`scripts/sign-bundled-plugins.sh`).
//! Each must be signed by the first key in `keys/release.pub` and hold exactly the committed
//! manifest and component. The secret key itself never reaches this build, nor any other build
//! script. `KEEFAST_REQUIRE_RELEASE_SIGNING=1` makes the build fail without them.
//!
//! Every other build signs the bundled Plugins with a key it makes now and throws away: its secret
//! half is never written anywhere, and keefast trusts it only for the packages compiled into that
//! build. `docs/plugins/signing.md` explains the trust model.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use keefast_plugin_package::{PublisherKey, SigningKey, pack, read, with_publisher_key};

/// The bundled Plugins: `plugins/<name>/` holds `plugin.json`, `<name>.wasm` and maybe
/// `icon.png`. Keep the list in step with `BUNDLED` in crates/app/ui/bundled-plugins.ts and in
/// scripts/sign-bundled-plugins.sh.
const BUNDLED: &[&str] = &["card"];

const SIGNED: &str = "KEEFAST_SIGNED_PLUGINS";
const REQUIRE_RELEASE: &str = "KEEFAST_REQUIRE_RELEASE_SIGNING";

fn main() {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let plugins = crate_dir.join("../../plugins");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let release_pub = crate_dir.join("keys/release.pub");
    println!("cargo:rerun-if-changed={}", release_pub.display());
    println!("cargo:rerun-if-env-changed={SIGNED}");
    println!("cargo:rerun-if-env-changed={REQUIRE_RELEASE}");

    let release_keys = read_release_keys(&release_pub);
    let signed = env::var_os(SIGNED)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from);
    if signed.is_none() && env::var(REQUIRE_RELEASE).is_ok_and(|value| value == "1") {
        panic!(
            "{REQUIRE_RELEASE} is set but {SIGNED} isn't: a release bundles Plugins signed with \
             keefast's release key (docs/plugins/signing.md)"
        );
    }
    // Made only when there are no signed packages: never written anywhere, gone after this build.
    let own_key = signed.is_none().then(|| {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("the OS gives random bytes");
        SigningKey::from_bytes(&seed)
    });
    let key = match &own_key {
        Some(key) => PublisherKey::of(key),
        None => *release_keys.first().unwrap_or_else(|| {
            panic!(
                "{SIGNED} is set but crates/plugins/keys/release.pub names no key \
                 (docs/plugins/signing.md)"
            )
        }),
    };

    let mut packages = String::new();
    for name in BUNDLED {
        let dir = plugins.join(name);
        let manifest_path = dir.join("plugin.json");
        let component_path = dir.join(format!("{name}.wasm"));
        let icon_path = dir.join("icon.png");
        // A path that doesn't exist would rerun this every build, signing with a new key.
        for path in [&manifest_path, &component_path, &icon_path] {
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
        let manifest = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|error| panic!("{}: {error}", manifest_path.display()));
        let manifest = with_publisher_key(&manifest, &key)
            .unwrap_or_else(|error| panic!("{}: {error}", manifest_path.display()));
        let component = fs::read(&component_path)
            .unwrap_or_else(|error| panic!("{}: {error}", component_path.display()));
        let icon = fs::read(&icon_path).ok();

        let package = match (&own_key, &signed) {
            (Some(own_key), _) => pack(&manifest, &component, icon.as_deref(), own_key)
                .unwrap_or_else(|error| panic!("packing the bundled Plugin {name}: {error}")),
            (None, Some(signed)) => {
                let path = signed.join(format!("{name}.kfplugin"));
                println!("cargo:rerun-if-changed={}", path.display());
                let bytes =
                    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                let package =
                    read(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                if package.key != key
                    || package.manifest_json != manifest
                    || package.component != component
                    || package.icon != icon
                {
                    panic!(
                        "{} isn't plugins/{name} signed with keefast's current release key",
                        path.display()
                    );
                }
                bytes
            }
            (None, None) => unreachable!("a key is made when there are no signed packages"),
        };
        let package_path = out.join(format!("{name}.kfplugin"));
        fs::write(&package_path, package).expect("OUT_DIR is writable");
        writeln!(
            packages,
            "    include_bytes!({:?}),",
            package_path.display().to_string()
        )
        .unwrap();
    }

    let release_keys: String = release_keys
        .iter()
        .map(|key| format!("{:?}, ", key.to_base64()))
        .collect();
    let code = format!(
        "// Made by build.rs.\n\
         /// The packages of the Plugins bundled with keefast, signed when keefast was built.\n\
         const BUNDLED_PACKAGES: &[&[u8]] = &[\n{packages}];\n\
         /// The public key they were signed with.\n\
         const BUNDLED_KEY: &str = {:?};\n\
         /// Whether that is keefast's release key, or a key this build made for itself.\n\
         const BUNDLED_BY_RELEASE_KEY: bool = {};\n\
         /// keefast's release keys (`keys/release.pub`), the one releases sign with first.\n\
         const RELEASE_KEYS: &[&str] = &[{release_keys}];\n",
        key.to_base64(),
        own_key.is_none(),
    );
    fs::write(out.join("bundled.rs"), code).expect("OUT_DIR is writable");
}

/// The keys in `keys/release.pub`: one base64 key a line, `#` starting a comment.
fn read_release_keys(path: &Path) -> Vec<PublisherKey> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty())
        .map(|line| {
            PublisherKey::parse(line)
                .unwrap_or_else(|error| panic!("{}: {line}: {error}", path.display()))
        })
        .collect()
}
