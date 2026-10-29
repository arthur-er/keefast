//! Locking Vaults: on demand, after inactivity, and when the OS locks or suspends (ADR 0004).
//!
//! A locked Vault drops everything decrypted (its keys and secrets are zeroized as they go)
//! and keeps only where its Vault file is, and where the user was in it. A Vault with unsaved
//! changes is not locked behind the user's back: as in KeePassXC, it is saved first when
//! autosave is on, and otherwise, or if saving fails, the user is asked whether to save or
//! discard the changes, or not to lock after all.

use std::time::Duration;

use keefast_core::GroupId;

use crate::clipboard::Clipboard;
use crate::{EntryScope, PendingSave, VaultId, VaultLocation, View, Workspace};

/// A Vault that is locked: only where its Vault file is, until it is unlocked again.
pub struct LockedVault {
    id: VaultId,
    location: VaultLocation,
    /// Where the user was in the Vault when it locked, to return them there.
    view: Option<View>,
    /// The Groups the Group shown was in when it locked, nearest first, to return the user to
    /// if that Group is gone when it is unlocked.
    shown_in: Vec<GroupId>,
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

    /// The locked Vault to offer for unlocking first: the one the user was looking at, or else
    /// the first locked.
    pub fn vault_to_unlock(&self) -> Option<VaultId> {
        let shown = self
            .view
            .vault()
            .filter(|&id| self.locked_vault(id).is_some());
        shown.or_else(|| self.locked.first().map(|locked| locked.id))
    }

    /// The global Lock: locks every unlocked Vault, and clears what keefast copied from the
    /// clipboard. Run the returned saves, of Vaults to save before they lock, as any other.
    pub fn lock_all(&mut self, clipboard: &mut dyn Clipboard) -> Vec<PendingSave> {
        let all = self.vaults.iter().map(|open| open.id).collect();
        self.request_lock(all, clipboard, false)
    }

    /// Locks one Vault, and clears what keefast copied from the clipboard, like
    /// [`Self::lock_all`].
    pub fn lock_vault(&mut self, id: VaultId, clipboard: &mut dyn Clipboard) -> Vec<PendingSave> {
        self.request_lock(vec![id], clipboard, false)
    }

    /// Tells the Workspace the OS screen locked or the machine is suspending: every Vault
    /// locks, unless the user turned that off. A Vault with unsaved changes can only be saved
    /// or discarded, not left open: whoever is at the keyboard didn't ask to lock it, and may
    /// not be the user.
    pub fn os_locked(&mut self, clipboard: &mut dyn Clipboard) -> Vec<PendingSave> {
        if !self.config.lock_on_os_lock {
            return Vec::new();
        }
        let all = self.vaults.iter().map(|open| open.id).collect();
        self.request_lock(all, clipboard, true)
    }

    /// How long without the user's input before every Vault locks; `None` never locks them.
    pub fn idle_lock_after(&self) -> Option<Duration> {
        self.config.idle_lock_after
    }

    /// Changes how long the Vaults wait for the user's input before they lock, counting from
    /// their last input, and remembers it in the app config.
    pub fn set_idle_lock_after(&mut self, after: Option<Duration>) {
        self.config.idle_lock_after = after;
        self.save_config();
    }

    pub fn lock_on_os_lock(&self) -> bool {
        self.config.lock_on_os_lock
    }

    /// Turns locking every Vault when the OS locks or suspends on or off, and remembers the
    /// choice in the app config.
    pub fn set_lock_on_os_lock(&mut self, lock: bool) {
        self.config.lock_on_os_lock = lock;
        self.save_config();
    }

    /// Tells the Workspace the user gave keefast input, which postpones the idle lock.
    pub fn note_activity(&mut self) {
        self.last_activity = self.clock.now();
    }

    /// How long until every Vault locks for inactivity, if one is unlocked and the idle lock is
    /// on. Call [`Self::lock_if_idle`] then.
    pub fn idle_lock_in(&self) -> Option<Duration> {
        let after = self.config.idle_lock_after?;
        if self
            .vaults
            .iter()
            .all(|open| open.lock_requested && open.auto_lock)
        {
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
            .filter(|open| !(open.lock_requested && open.auto_lock))
            .map(|open| open.id)
            .collect();
        self.request_lock(due, clipboard, true)
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
            // Only a lock the user asked for can be called off.
            if open.auto_lock {
                return None;
            }
            open.lock_requested = false;
        }
        // Answering is the user's input, so the idle lock doesn't ask again at once.
        self.note_activity();
        match answer {
            UnsavedLock::Save => self.begin_save(id).ok(),
            UnsavedLock::Discard => {
                self.lock_now(&[id]);
                None
            }
            UnsavedLock::Cancel => None,
        }
    }

    /// Whether the user may call off the lock of a Vault waiting to lock: not when idle time,
    /// the OS locking or a suspend asked for it.
    pub fn can_cancel_lock(&self, id: VaultId) -> bool {
        self.vault(id)
            .is_some_and(|open| open.lock_requested && !open.auto_lock)
    }

    /// Locks the Vaults that have nothing unsaved, and marks the others to lock once they
    /// are saved, saving them now with autosave on. A Vault marked is hidden from the browser
    /// and the app at once, and browser requests waiting on it are answered as locked. `auto`
    /// is for locks the user didn't ask for, which can't be cancelled.
    fn request_lock(
        &mut self,
        ids: Vec<VaultId>,
        clipboard: &mut dyn Clipboard,
        auto: bool,
    ) -> Vec<PendingSave> {
        self.pending_clear.clear_now(clipboard);
        let mut unsaved = Vec::new();
        let mut saved = Vec::new();
        for id in ids {
            let Some(open) = self.vault_mut(id) else {
                continue;
            };
            if open.has_unsaved_changes() {
                open.lock_requested = true;
                open.auto_lock |= auto;
                unsaved.push(id);
                self.cut_off_browser_requests(id);
            } else {
                saved.push(id);
            }
        }
        self.lock_now(&saved);
        if !self.config.autosave {
            return Vec::new();
        }
        unsaved
            .into_iter()
            .filter_map(|id| self.begin_save(id).ok())
            .collect()
    }

    /// Locks Vaults now, dropping their decrypted contents. Browser requests waiting on them
    /// are answered that they are locked, and the extensions are told once.
    pub(crate) fn lock_now(&mut self, ids: &[VaultId]) {
        let before = self.locked.len();
        for &id in ids {
            self.lock_one(id);
        }
        if self.locked.len() > before {
            self.announce_locked();
            self.forget_unused_favicons();
        }
    }

    fn lock_one(&mut self, id: VaultId) {
        let Some(at) = self.vaults.iter().position(|open| open.id == id) else {
            return;
        };
        self.cut_off_browser_requests(id);
        let shown_in = self.groups_shown_in(id);
        let open = self.vaults.remove(at);
        let view = self.leave_vault(id);
        let at = self.locked.partition_point(|locked| locked.id < id);
        self.locked.insert(
            at,
            LockedVault {
                id,
                location: open.location,
                view,
                shown_in,
            },
        );
    }

    /// Takes what the user is shown of a Vault that is locking out of the view, and returns
    /// the view as it was if it showed any of it.
    fn leave_vault(&mut self, id: VaultId) -> Option<View> {
        let before = self.view.clone();
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
    /// in it and the Groups the Group shown was in.
    pub(crate) fn take_locked(&mut self, id: VaultId) -> Option<(View, Vec<GroupId>)> {
        let at = self.locked.iter().position(|locked| locked.id == id)?;
        let locked = self.locked.remove(at);
        Some((locked.view?, locked.shown_in))
    }

    /// Returns the user to where they were, as far as it is still there (see
    /// [`Workspace::keep_view_to_what_exists`]).
    pub(crate) fn restore_view(&mut self, view: View, shown_in: &[GroupId]) {
        self.view = view;
        self.keep_view_to_what_exists(shown_in);
    }
}
