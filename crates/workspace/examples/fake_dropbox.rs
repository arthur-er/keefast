//! A fake Dropbox to try keefast's Dropbox support without a Dropbox account. It lets keefast
//! in as soon as the browser opens its authorize page, and holds a few Vault files (master
//! password `fixture-password`).
//!
//! ```sh
//! cargo run -p keefast-workspace --example fake_dropbox
//! ```
//!
//! then start keefast with the environment it prints. Type `change` and Enter to have another
//! device add a Group to every Vault file, for keefast to find and Merge in.

#[path = "../tests/support/fake_dropbox.rs"]
#[allow(dead_code)]
mod fake_dropbox;

use std::io::BufRead;

use fake_dropbox::{APP_KEY, FakeDropbox};
use keefast_core::{CompositeKey, Timestamp, Vault};

const VAULT: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
const PASSWORD: &str = "fixture-password";
const VAULTS: [&str; 3] = [
    "/Vaults/Personal.kdbx",
    "/Vaults/Work/Work.kdbx",
    "/Shared/Family.kdbx",
];

fn main() {
    let fake = FakeDropbox::start();
    for path in VAULTS {
        fake.add_file(path, VAULT);
    }
    fake.add_file("/Vaults/Recovery codes.txt", b"not a Vault file");
    fake.add_folder("/Photos");
    let url = fake.url();
    println!("Fake Dropbox at {url}. Start keefast with:");
    println!(
        "KEEFAST_DROPBOX_APP_KEY={APP_KEY} KEEFAST_DROPBOX_API_URL={url} \
         KEEFAST_DROPBOX_CONTENT_URL={url} KEEFAST_DROPBOX_AUTHORIZE_URL={url}/oauth2/authorize"
    );
    let mut changes = 0;
    for line in std::io::stdin().lock().lines() {
        if line.is_ok_and(|line| line.trim() == "change") {
            changes += 1;
            for path in VAULTS {
                change_elsewhere(&fake, path, &format!("From another device {changes}"));
            }
            println!("Another device added a Group to every Vault file.");
        }
    }
    // Without a terminal, keep serving.
    loop {
        std::thread::park();
    }
}

/// Another device adds a Group to the Vault file and uploads it.
fn change_elsewhere(fake: &FakeDropbox, path: &str, group: &str) {
    let key = CompositeKey::password(PASSWORD);
    let mut vault = Vault::open(&fake.file(path).unwrap(), &key).expect("the Vault file opens");
    let root = vault.root().id();
    let now = Timestamp::from_system_time(std::time::SystemTime::now());
    vault.create_group(root, group, now).unwrap();
    fake.add_file(path, &vault.save().unwrap());
}
