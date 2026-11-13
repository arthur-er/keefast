//! How long the app's background work waits: for a changed Vault file to settle before it is
//! read, and between looks at whether a sync or the idle lock is due. The app's threads only
//! sleep for these waits and call back into the Workspace.

use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use crate::{VaultId, Workspace};

impl Workspace {
    /// How long a Vault file must stay unchanged before it is read: sync tools write in steps.
    pub const VAULT_FILE_SETTLE: Duration = Duration::from_millis(500);
    /// The longest wait between looks at whether a sync is due: a Vault may be opened or
    /// unlocked meanwhile, with a save made offline to upload at once.
    pub const SYNC_LOOK_AT_MOST: Duration = Duration::from_secs(5);
    /// The longest wait between looks at whether the idle lock is due: a Vault may be unlocked
    /// meanwhile.
    pub const IDLE_LOCK_LOOK_AT_MOST: Duration = Duration::from_secs(60);

    /// How long to wait before looking again whether a sync is due: until the next is, or
    /// [`Self::SYNC_LOOK_AT_MOST`] if that is sooner or none is. Call [`Self::begin_syncs`]
    /// once [`Self::sync_in`] is zero.
    pub fn sync_wait(&self) -> Duration {
        at_most(self.sync_in(), Self::SYNC_LOOK_AT_MOST)
    }

    /// How long to wait before looking again whether the idle lock is due: until it is, or
    /// [`Self::IDLE_LOCK_LOOK_AT_MOST`] if that is sooner or it isn't on. Call
    /// [`Self::lock_if_idle`] once [`Self::idle_lock_in`] is zero.
    pub fn idle_lock_wait(&self) -> Duration {
        at_most(self.idle_lock_in(), Self::IDLE_LOCK_LOOK_AT_MOST)
    }
}

fn at_most(due_in: Option<Duration>, most: Duration) -> Duration {
    due_in.unwrap_or(most).min(most)
}

/// Waits for the next Vault file change that [`Workspace::watch_vault_files`] passes on
/// through `changed`, then until changes stop coming for [`Workspace::VAULT_FILE_SETTLE`], and
/// returns every Vault whose Vault file changed meanwhile, for
/// [`Workspace::vault_file_changed`]. Returns `None` once nothing can send any more.
pub fn settled_vault_file_changes(changed: &Receiver<VaultId>) -> Option<Vec<VaultId>> {
    settled(changed, Workspace::VAULT_FILE_SETTLE)
}

fn settled(changed: &Receiver<VaultId>, settle: Duration) -> Option<Vec<VaultId>> {
    let mut vaults = vec![changed.recv().ok()?];
    loop {
        thread::sleep(settle);
        let more: Vec<_> = changed.try_iter().collect();
        if more.is_empty() {
            return Some(vaults);
        }
        vaults.extend(more);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    const SETTLE: Duration = Duration::from_millis(50);

    #[test]
    fn a_change_is_passed_on_once_the_file_has_settled() {
        let (changes, changed) = mpsc::channel();
        changes.send(VaultId(1)).unwrap();
        let started = Instant::now();

        assert_eq!(settled(&changed, SETTLE), Some(vec![VaultId(1)]));
        assert!(started.elapsed() >= SETTLE);
    }

    #[test]
    fn changes_in_steps_are_passed_on_together_once_they_stop() {
        let (changes, changed) = mpsc::channel();
        let writer = thread::spawn(move || {
            for vault in [1, 2, 1] {
                changes.send(VaultId(vault)).unwrap();
                thread::sleep(Duration::from_millis(10));
            }
            changes
        });

        // Settling for long enough that a slow test machine still writes every step in time.
        let vaults = settled(&changed, Duration::from_millis(250)).unwrap();
        let _changes = writer.join().unwrap();

        assert_eq!(vaults, vec![VaultId(1), VaultId(2), VaultId(1)]);
        assert!(changed.try_recv().is_err(), "nothing is left for later");
    }

    #[test]
    fn nothing_is_passed_on_once_watching_ends() {
        let (changes, changed) = mpsc::channel::<VaultId>();
        drop(changes);

        assert_eq!(settled(&changed, SETTLE), None);
    }

    #[test]
    fn looks_are_never_further_apart_than_at_most() {
        let most = Duration::from_secs(5);
        assert_eq!(at_most(None, most), most);
        assert_eq!(at_most(Some(Duration::from_secs(30)), most), most);
        assert_eq!(
            at_most(Some(Duration::from_secs(2)), most),
            Duration::from_secs(2)
        );
        assert_eq!(at_most(Some(Duration::ZERO), most), Duration::ZERO);
    }
}
