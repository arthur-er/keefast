//! Makes a Plugin package (`.kfplugin`, see `PACKAGE.md`) and the key to sign it with.
//!
//! ```sh
//! cargo run -p keefast-plugins --example pack -- new-key publisher.key
//! cargo run -p keefast-plugins --example pack -- pack publisher.key plugin.json plugin.wasm [icon.png] out.kfplugin
//! ```
//!
//! `new-key` writes a new ed25519 secret key (base64) and prints its public key, for the
//! manifest's `publisherKey`. Keep the secret key: every update of the Plugin must be signed with
//! it, or keefast refuses the update.

use std::fs;
use std::process::ExitCode;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::SigningKey;
use keefast_plugins::PublisherKey;
use keefast_plugins::package::pack;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["new-key", path] => new_key(path),
        ["pack", key, manifest, component, out] => make(key, manifest, component, None, out),
        ["pack", key, manifest, component, icon, out] => {
            make(key, manifest, component, Some(icon), out)
        }
        _ => Err(
            "usage: pack new-key <key file> | pack pack <key file> <plugin.json> \
                  <component.wasm> [icon.png] <out.kfplugin>"
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn new_key(path: &str) -> Result<(), String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| error.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    fs::write(path, BASE64.encode(key.to_bytes())).map_err(|error| error.to_string())?;
    println!("publisherKey: {}", PublisherKey::of(&key).to_base64());
    Ok(())
}

fn make(
    key: &str,
    manifest: &str,
    component: &str,
    icon: Option<&str>,
    out: &str,
) -> Result<(), String> {
    let read = |path: &str| fs::read(path).map_err(|error| format!("{path}: {error}"));
    let seed: [u8; 32] = BASE64
        .decode(read(key)?.trim_ascii())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or("the key file isn't a key made by new-key")?;
    let manifest = String::from_utf8(read(manifest)?).map_err(|error| error.to_string())?;
    let icon = icon.map(read).transpose()?;
    let package = pack(
        &manifest,
        &read(component)?,
        icon.as_deref(),
        &SigningKey::from_bytes(&seed),
    )
    .map_err(|error| error.to_string())?;
    fs::write(out, &package).map_err(|error| error.to_string())?;
    println!("wrote {out} ({} bytes)", package.len());
    Ok(())
}
