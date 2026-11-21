//! Editing an Entry as the app's editor and its quick actions do: a partial change applied to the
//! Entry as it is when the edit lands, so an unrevealed secret is kept and a Merge that came in
//! meanwhile isn't undone. An edit that changes nothing leaves the Vault saved.

mod support;

use std::sync::Arc;
use std::time::Duration;

use keefast_core::{EntryFields, EntryId};
use keefast_workspace::Workspace;
use secrecy::SecretString;
use support::{FakeClock, FakeStorage, KEEPASSXC_KDBX4, PASSWORD, personal_vault};

const START: u64 = 1_800_000_000;

struct Harness {
    clock: Arc<FakeClock>,
    workspace: Workspace,
    vault: keefast_workspace::VaultId,
}

fn harness() -> Harness {
    let storage = Arc::new(FakeStorage::default().with_file(&personal_vault(), KEEPASSXC_KDBX4));
    let clock = Arc::new(FakeClock::at_unix_seconds(START));
    let mut workspace = Workspace::with_clock(storage, clock.clone());
    let vault = workspace
        .open_vault(
            &personal_vault(),
            &keefast_workspace::UnlockSecrets::password(PASSWORD),
        )
        .expect("fixture opens");
    Harness {
        clock,
        workspace,
        vault,
    }
}

impl Harness {
    /// A new Entry, saved, so the Vault starts with nothing unsaved.
    fn saved_entry(&mut self, fields: &EntryFields) -> EntryId {
        let root = self
            .workspace
            .vault(self.vault)
            .unwrap()
            .vault()
            .root()
            .id();
        let id = self
            .workspace
            .create_entry(self.vault, root, fields)
            .unwrap();
        self.workspace.save_vault(self.vault).unwrap();
        self.clock.advance(Duration::from_secs(60));
        id
    }

    fn unsaved(&self) -> bool {
        self.workspace
            .vault(self.vault)
            .unwrap()
            .has_unsaved_changes()
    }

    fn history(&self, entry: EntryId) -> usize {
        self.workspace
            .entry(self.vault, entry)
            .unwrap()
            .history()
            .len()
    }
}

fn bank() -> EntryFields {
    EntryFields {
        title: "Bank".into(),
        password: SecretString::from("made-here"),
        ..EntryFields::default()
    }
}

#[test]
fn setting_the_fields_an_entry_already_has_leaves_the_vault_saved() {
    let mut h = harness();
    let id = h.saved_entry(&bank());

    let same = EntryFields::from(h.workspace.entry(h.vault, id).unwrap());
    h.workspace.update_entry(h.vault, id, &same).unwrap();

    assert!(!h.unsaved(), "nothing changed, so nothing is left to save");
    assert_eq!(h.history(id), 0);
}
