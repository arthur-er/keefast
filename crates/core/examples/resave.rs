//! Opens a Vault file and saves it again, for checking keefast's saves with other KeePass
//! clients (scripts/check-interop.sh).
//!
//! Usage: cargo run --example resave -- <in.kdbx> <out.kdbx> <password>

use keefast_core::{CompositeKey, Vault};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let [_, source, target, password] =
        <[String; 4]>::try_from(std::env::args().collect::<Vec<_>>())
            .map_err(|_| "usage: resave <in.kdbx> <out.kdbx> <password>")?;
    let vault = Vault::open(&std::fs::read(source)?, &CompositeKey::password(password))?;
    std::fs::write(target, vault.save()?)?;
    Ok(())
}
