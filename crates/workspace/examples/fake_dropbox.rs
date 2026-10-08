//! A fake Dropbox to try keefast's Dropbox support without a Dropbox account. It lets keefast
//! in as soon as the browser opens its authorize page, and holds a few Vault files (master
//! password `fixture-password`).
//!
//! ```sh
//! cargo run -p keefast-workspace --example fake_dropbox
//! ```
//!
//! then start keefast with the environment it prints.

#[path = "../tests/support/fake_dropbox.rs"]
#[allow(dead_code)]
mod fake_dropbox;

use fake_dropbox::{APP_KEY, FakeDropbox};

const VAULT: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");

fn main() {
    let fake = FakeDropbox::start();
    fake.add_file("/Vaults/Personal.kdbx", VAULT);
    fake.add_file("/Vaults/Work/Work.kdbx", VAULT);
    fake.add_file("/Vaults/Recovery codes.txt", b"not a Vault file");
    fake.add_file("/Shared/Family.kdbx", VAULT);
    fake.add_folder("/Photos");
    let url = fake.url();
    println!("Fake Dropbox at {url}. Start keefast with:");
    println!(
        "KEEFAST_DROPBOX_APP_KEY={APP_KEY} KEEFAST_DROPBOX_API_URL={url} \
         KEEFAST_DROPBOX_CONTENT_URL={url} KEEFAST_DROPBOX_AUTHORIZE_URL={url}/oauth2/authorize"
    );
    loop {
        std::thread::park();
    }
}
