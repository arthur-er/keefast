//! Locking Vaults: on demand, after inactivity, and when the OS locks or suspends (ADR 0004).
//!
//! A locked Vault drops everything decrypted (its keys and secrets are zeroized as they go)
//! and keeps only where its Vault file is, and where the user was in it. A Vault with unsaved
//! changes is not locked behind the user's back: as in KeePassXC, it is saved first when
//! autosave is on, and otherwise, or if saving fails, the user is asked whether to save or
//! discard the changes, or not to lock after all.

use std::time::Duration;

use crate::clipboard::Clipboard;
use crate::{EntryScope, PendingSave, VaultId, VaultLocation, View, Workspace};

/// A Vault that is locked: only where its Vault file is, until it is unlocked again.
pub struct LockedVault {
    id: VaultId,
    location: VaultLocation,
    /// Where the user was in the Vault when it locked, to return them there.
    view: Option<View>,
}

impl LockedVault {
    pub fn id(&self) -> VaultId {
        self.id
    }

    pub fn location(&self) -> &VaultLocation {
        &self.location
    }

    pub fn display_name(&self) -> String {
        self.location.display_name()
    }
}

/// The user's answer when a Vault to lock has unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsavedLock {
    /// Save the changes, then lock.
    Save,
    /// Lock without saving: the changes are lost.
    Discard,
    /// Don't lock the Vault.
    Cancel,
}

impl Workspace {
    /// The locked Vaults, in the order they were opened.
    pub fn locked_vaults(&self) -> impl Iterator<Item = &LockedVault> {
        self.locked.iter()
    }

    pub fn locked_vault(&self, id: VaultId) -> Option<&LockedVault> {
        self.locked.iter().find(|locked| locked.id == id)
    }

    /// The locked Vault read from the Vault file at `location`, if there is one.
    pub fn locked_vault_at(&self, location: &VaultLocation) -> Option<VaultId> {
        self.locked
            .iter()
            .find(|locked| &locked.location == location)
            .map(|locked| locked.id)
    }

    /// The global Lock: locks every unlocked Vault, and clears what keefast copied from the
    /// clipboard. Run the returned saves, of Vaults to save before they lock, as any other.
    pub fn lock_all(&mut self, clipboard: &mut dyn Clipboard) -> Vec<PendingSave> {
        let all = self.vaults.iter().map(|open| open.id).collect();
        self.request_lock(all, clipboard)
    }

    /// Locks one Vault, and clears what keefast copied from the clipboard, like
    /// [`Self::lock_all`].
    pub fn lock_vault(&mut self, id: VaultId, clipboard: &mut dyn Clipboard) -> Vec<PendingSave> {
        self.request_lock(vec![id], clipboard)
    }

    /// Whether every Vault locks when the OS screen locks or the machine suspends.
    pub fn lock_on_os_lock(&self) -> bool {
        self.config.lock_on_os_lock
    }

    /// Tells the Workspace the OS screen locked or the machine is suspending: every Vault
    /// locks, unless the user turned that off.
    pub fn os_locked(&mut self, clipboard: &mut dyn Clipboard) -> Vec<PendingSave> {
        if !self.config.lock_on_os_lock {
            return Vec::new();
        }
        self.lock_all(clipboard)
    }

    /// Tells the Workspace the user gave keefast input, which postpones the idle lock.
    pub fn note_activity(&mut self) {
        self.last_activity = self.clock.now();
    }

    /// How long until every Vault locks for inactivity, if one is unlocked and the idle lock is
    /// on. Call [`Self::lock_if_idle`] then.
    pub fn idle_lock_in(&self) -> Option<Duration> {
        let after = self.config.idle_lock_after?;
        if self.vaults.iter().all(|open| open.lock_requested) {
            return None;
        }
        let idle = self
            .clock
            .now()
            .duration_since(self.last_activity)
            .unwrap_or_default();
        Some(after.saturating_sub(idle))
    }

    /// Locks every Vault, like [`Self::lock_all`], once the user has been inactive for the
    /// configured wait.
    pub fn lock_if_idle(&mut self, clipboard: &mut dyn Clipboard) -> Vec<PendingSave> {
        if self.idle_lock_in() != Some(Duration::ZERO) {
            return Vec::new();
        }
        let due = self
            .vaults
            .iter()
            .filter(|open| !open.lock_requested)
            .map(|open| open.id)
            .collect();
        self.request_lock(due, clipboard)
    }

    /// The Vault waiting to lock until the user says what to do with its unsaved changes, if
    /// one is.
    pub fn pending_lock(&self) -> Option<VaultId> {
        self.vaults
            .iter()
            .find(|open| open.lock_requested && !open.saving && open.has_unsaved_changes())
            .map(|open| open.id)
    }

    /// Answers whether to save the unsaved changes of a Vault waiting to lock. Saving returns
    /// the save to run, after which the Vault locks; if the save fails, the user is asked
    /// again.
    pub fn answer_lock(&mut self, id: VaultId, answer: UnsavedLock) -> Option<PendingSave> {
        let open = self.vault_mut(id).filter(|open| open.lock_requested)?;
        if answer == UnsavedLock::Cancel {
            open.lock_requested = false;
        }
        // Answering is the user's input, so the idle lock doesn't ask again at once.
        self.note_activity();
        match answer {
            UnsavedLock::Save => self.begin_save(id).ok(),
            UnsavedLock::Discard => {
                self.lock_now(id);
                None
            }
            UnsavedLock::Cancel => None,
        }
    }

    /// Locks the Vaults that have nothing unsaved, and marks the others to lock once they
    /// are saved, saving them now with autosave on.
    fn request_lock(
        &mut self,
        ids: Vec<VaultId>,
        clipboard: &mut dyn Clipboard,
    ) -> Vec<PendingSave> {
        self.pending_clear.clear_now(clipboard);
        let mut unsaved = Vec::new();
        for id in ids {
            let Some(open) = self.vault_mut(id) else {
                continue;
            };
            if open.has_unsaved_changes() {
                open.lock_requested = true;
                unsaved.push(id);
            } else {
                self.lock_now(id);
            }
        }
        if !self.config.autosave {
            return Vec::new();
        }
        unsaved
            .into_iter()
            .filter_map(|id| self.begin_save(id).ok())
            .collect()
    }

    /// Locks a Vault now, dropping its decrypted contents. Browser requests waiting on it are
    /// answered that it is locked.
    pub(crate) fn lock_now(&mut self, id: VaultId) {
        let Some(at) = self.vaults.iter().position(|open| open.id == id) else {
            return;
        };
        self.cut_off_browser_requests(id);
        let open = self.vaults.remove(at);
        let view = self.leave_vault(id);
        let at = self.locked.partition_point(|locked| locked.id < id);
        self.locked.insert(
            at,
            LockedVault {
                id,
                location: open.location,
                view,
            },
        );
        self.announce_locked();
    }

    /// Takes what the user is shown of a Vault that is locking out of the view, and returns
    /// the view as it was if it showed any of it.
    fn leave_vault(&mut self, id: VaultId) -> Option<View> {
        let before = self.view;
        if let EntryScope::Group(vault, _) = self.view.scope
            && vault == id
        {
            self.view.scope = EntryScope::Vault(id);
        }
        if self.view.entry.is_some_and(|(vault, _)| vault == id) {
            self.view.entry = None;
        }
        let shown = before.vault() == Some(id) || before.entry.is_some_and(|(v, _)| v == id);
        shown.then_some(before)
    }

    /// Takes a locked Vault out of the locked ones, to be unlocked, with where the user was
    /// in it.
    pub(crate) fn take_locked(&mut self, id: VaultId) -> Option<View> {
        let at = self.locked.iter().position(|locked| locked.id == id)?;
        self.locked.remove(at).view
    }

    /// Returns the user to where they were, as far as it is still there: a Group or Entry gone
    /// meanwhile, or in a Vault still locked, is left out.
    pub(crate) fn restore_view(&mut self, mut view: View) {
        if let EntryScope::Group(vault, group) = view.scope
            && self.group(vault, group).is_none()
        {
            view.scope = EntryScope::Vault(vault);
        }
        if let Some((vault, entry)) = view.entry
            && self.entry(vault, entry).is_none()
        {
            view.entry = None;
        }
        self.view = view;
    }
}
