//! Merges one version of a Vault file into another and saves the result, for checking
//! keefast's Merge against other KeePass clients' (scripts/check-merge-interop.sh).
//!
//! Usage: cargo run --example merge -- <ours.kdbx> <theirs.kdbx> <out.kdbx> <password>

use keefast_core::{CompositeKey, Vault};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let [_, ours, theirs, target, password] =
        <[String; 5]>::try_from(std::env::args().collect::<Vec<_>>())
            .map_err(|_| "usage: merge <ours.kdbx> <theirs.kdbx> <out.kdbx> <password>")?;
    let mut vault = Vault::open(&std::fs::read(ours)?, &CompositeKey::password(password))?;
    let theirs = vault.open_version(&std::fs::read(theirs)?)?;
    let summary = vault.merge(&theirs);
    eprintln!("{summary:?}");
    std::fs::write(target, vault.save()?)?;
    Ok(())
}
