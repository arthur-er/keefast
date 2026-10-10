//! The app's state: the Workspace behind a mutex, and everything that must happen after it
//! changes. Every change goes through [`AppCore::update`], which, once the Workspace is let go
//! of, sends the browsers what the Workspace has for them, starts favicon downloads, tells the UI
//! about Merges and browser requests, and emits `WorkspaceChanged` with a new revision. Saves and
//! other slow work run on threads of their own, never holding the Workspace while they wait.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use keefast_workspace::{
    Clipboard, EditVaultError, Outgoing, PendingFavicon, PendingSave, VaultId, Workspace,
};

use crate::browser::BrowserBridge;
use crate::clipboard::SystemClipboard;
use crate::error::{ApiError, ApiResult};
use crate::events::{
    AppEvent, BrowserRequest, BrowserRequestKind, ClipboardCleared, MergeNotice, Notifier,
    SaveFailed, WorkspaceChanged,
};
use crate::merge_notice;

/// What the command line asked to open first.
#[derive(Debug, Clone, Default)]
pub struct Launch {
    pub vault_file: Option<PathBuf>,
    pub key_file: Option<PathBuf>,
}

/// Whether a failed save is announced with a `SaveFailed` event, or only shows in the save
/// status: background syncs and autosaves fail quietly, or an offline Dropbox would announce
/// itself every few minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failures {
    Announce,
    Quiet,
}

pub struct AppCore {
    workspace: Mutex<Workspace>,
    clipboard: Mutex<SystemClipboard>,
    revision: AtomicU32,
    notifier: Box<dyn Notifier>,
    /// Where favicon downloads are queued for the download threads, once they run.
    favicons: Mutex<Option<std::sync::mpsc::Sender<PendingFavicon>>>,
    pub(crate) browser: Mutex<BrowserBridge>,
    /// The browser requests the UI was last told about.
    shown_requests: Mutex<Requests>,
    /// Counts copies, so only the latest copy's clipboard timer keeps running.
    copies: AtomicU64,
    launch: Launch,
}

/// The browser requests waiting for the user, by number: the one put to the user now of each
/// kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Requests {
    pairing: Option<u64>,
    access: Option<u64>,
    login: Option<u64>,
}

impl Requests {
    fn of(workspace: &Workspace) -> Self {
        Self {
            pairing: workspace
                .pending_pairing()
                .map(|pending| pending.id.as_u64()),
            access: workspace
                .pending_access()
                .map(|pending| pending.id.as_u64()),
            login: workspace.pending_login().map(|pending| pending.id.as_u64()),
        }
    }

    /// The request put to the user now: a Pairing request over an Access request over a
    /// captured login, as the GPUI app asked them.
    fn front(self) -> Option<(BrowserRequestKind, u64)> {
        let kinds = [
            (BrowserRequestKind::Pairing, self.pairing),
            (BrowserRequestKind::Access, self.access),
            (BrowserRequestKind::Login, self.login),
        ];
        kinds
            .into_iter()
            .find_map(|(kind, id)| id.map(|id| (kind, id)))
    }

    /// The kind of request to put to the user now, if it isn't the one put to them `before`.
    fn news_since(self, before: Self) -> Option<BrowserRequestKind> {
        let front = self.front()?;
        (Some(front) != before.front()).then_some(front.0)
    }
}

/// What an update leaves to do once the Workspace is let go of.
struct Aftermath {
    messages: Vec<Outgoing>,
    favicons: Vec<PendingFavicon>,
    merges: Vec<MergeNotice>,
    requests: Requests,
    clipboard_cleared: bool,
}

impl AppCore {
    pub fn new(workspace: Workspace, notifier: Box<dyn Notifier>, launch: Launch) -> Arc<Self> {
        Arc::new(Self {
            workspace: Mutex::new(workspace),
            clipboard: Mutex::new(SystemClipboard::default()),
            revision: AtomicU32::new(0),
            notifier,
            favicons: Mutex::new(None),
            browser: Mutex::new(BrowserBridge::default()),
            shown_requests: Mutex::new(Requests::default()),
            copies: AtomicU64::new(0),
            launch,
        })
    }

    pub fn launch(&self) -> &Launch {
        &self.launch
    }

    /// The revision the UI was last told about.
    pub fn revision(&self) -> u32 {
        self.revision.load(Ordering::SeqCst)
    }

    fn workspace(&self) -> MutexGuard<'_, Workspace> {
        // A panic while holding it leaves the Workspace as consistent as it was: go on.
        self.workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads the Workspace. Changes nothing, so tells the UI nothing.
    pub fn read<R>(&self, read: impl FnOnce(&Workspace) -> R) -> R {
        read(&self.workspace())
    }

    /// Changes the Workspace in a way nothing outside it sees, such as noting the user's
    /// activity or remembering pane widths: the UI and the browsers aren't told.
    pub fn update_quietly<R>(&self, change: impl FnOnce(&mut Workspace) -> R) -> R {
        change(&mut self.workspace())
    }

    /// Changes the Workspace, then tells the browsers and the UI what changed.
    pub fn update<R>(&self, change: impl FnOnce(&mut Workspace) -> R) -> R {
        self.update_with_clipboard(|workspace, _| change(workspace))
    }

    /// [`Self::update`] for changes that copy to or clear the clipboard.
    pub fn update_with_clipboard<R>(
        &self,
        change: impl FnOnce(&mut Workspace, &mut dyn Clipboard) -> R,
    ) -> R {
        let (result, after) = {
            let mut workspace = self.workspace();
            let had_copy = workspace.clipboard_clears_in().is_some();
            let result = {
                let mut clipboard = self
                    .clipboard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                change(&mut workspace, &mut *clipboard)
            };
            (result, Self::aftermath(&mut workspace, had_copy))
        };
        self.dispatch(after);
        result
    }

    fn aftermath(workspace: &mut Workspace, had_copy: bool) -> Aftermath {
        let merges = workspace
            .take_merges()
            .into_iter()
            .map(|merge| {
                // The Vault may have locked since it was Merged.
                let name = vault_name(workspace, merge.vault);
                MergeNotice {
                    vault: merge.vault.into(),
                    message: merge_notice::describe(&name, &merge.summary),
                    vault_name: name,
                }
            })
            .collect();
        Aftermath {
            messages: workspace.take_browser_messages(),
            favicons: workspace.begin_favicon_downloads(),
            merges,
            requests: Requests::of(workspace),
            clipboard_cleared: had_copy && workspace.clipboard_clears_in().is_none(),
        }
    }

    fn dispatch(&self, after: Aftermath) {
        self.browser
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(after.messages);
        if !after.favicons.is_empty()
            && let Some(queue) = &*self.favicons.lock().unwrap_or_else(PoisonError::into_inner)
        {
            for download in after.favicons {
                let _ = queue.send(download);
            }
        }
        let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.notifier
            .emit(AppEvent::WorkspaceChanged(WorkspaceChanged { revision }));
        for merge in after.merges {
            self.notifier.emit(AppEvent::MergeNotice(merge));
        }
        let news = {
            let mut shown = self
                .shown_requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let news = after.requests.news_since(*shown);
            *shown = after.requests;
            news
        };
        if let Some(kind) = news {
            self.notifier
                .emit(AppEvent::BrowserRequest(BrowserRequest { kind }));
            self.notifier.bring_to_front();
        }
        if after.clipboard_cleared {
            self.notifier
                .emit(AppEvent::ClipboardCleared(ClipboardCleared {}));
        }
    }

    /// Hands favicon downloads to the download threads from now on.
    pub(crate) fn queue_favicons_to(&self, queue: std::sync::mpsc::Sender<PendingFavicon>) {
        *self.favicons.lock().unwrap_or_else(PoisonError::into_inner) = Some(queue);
    }

    /// Runs an edit, then autosaves if it went through.
    pub fn edit<T>(
        self: &Arc<Self>,
        edit: impl FnOnce(&mut Workspace) -> Result<T, EditVaultError>,
    ) -> ApiResult<T> {
        let result = self.update(edit);
        if result.is_ok() {
            self.autosave();
        }
        result.map_err(ApiError::from)
    }

    /// With autosave on, saves every Vault with unsaved changes, and Merges in changes made to
    /// Vault files elsewhere. Call it after edits.
    pub fn autosave(self: &Arc<Self>) {
        let saves = self.update(Workspace::begin_autosaves);
        self.run_saves(saves, Failures::Quiet);
    }

    /// Encrypts and writes off the caller's thread, each save on a thread of its own; edits
    /// made meanwhile are autosaved afterwards.
    pub fn run_saves(self: &Arc<Self>, saves: Vec<PendingSave>, failures: Failures) {
        for save in saves {
            let core = Arc::clone(self);
            thread::spawn(move || {
                let vault = save.vault_id();
                let finished = save.run();
                // A failure shows in the save status; the Vault stays unsaved.
                let failed = core.update(|workspace| {
                    let result = workspace.finish_save(finished);
                    result
                        .err()
                        .map(|error| (vault_name(workspace, vault), ApiError::from(error)))
                });
                if let (Some((vault_name, error)), Failures::Announce) = (failed, failures) {
                    core.notifier.emit(AppEvent::SaveFailed(SaveFailed {
                        vault: vault.into(),
                        vault_name,
                        message: error.message,
                        detail: error.detail,
                    }));
                }
                core.autosave();
            });
        }
    }

    /// Locks with `lock`, then runs the saves of the Vaults to save before they lock.
    pub fn lock_with(
        self: &Arc<Self>,
        lock: impl FnOnce(&mut Workspace, &mut dyn Clipboard) -> Vec<PendingSave>,
    ) {
        let saves = self.update_with_clipboard(lock);
        self.run_saves(saves, Failures::Quiet);
    }

    /// Starts the timer that clears what was just copied once its time comes, replacing the
    /// timer of an earlier copy.
    pub fn clear_clipboard_when_due(self: &Arc<Self>) {
        let copy = self.copies.fetch_add(1, Ordering::SeqCst) + 1;
        let core = Arc::clone(self);
        thread::spawn(move || {
            while let Some(wait) = core.read(Workspace::clipboard_clears_in) {
                thread::sleep(wait);
                if core.copies.load(Ordering::SeqCst) != copy {
                    return;
                }
                core.update_with_clipboard(|workspace, clipboard| {
                    workspace.clear_clipboard_if_due(clipboard)
                });
            }
        });
    }

    /// Clears what keefast copied without waiting, as when quitting, so nothing copied outlives
    /// the app.
    pub fn clear_clipboard_now(&self) {
        self.update_with_clipboard(|workspace, clipboard| workspace.clear_clipboard_now(clipboard));
    }
}

/// What a Vault is called, unlocked or locked; empty once it is gone.
pub fn vault_name(workspace: &Workspace, vault: VaultId) -> String {
    workspace
        .vault(vault)
        .map(|open| open.display_name())
        .or_else(|| {
            workspace
                .locked_vault(vault)
                .map(|locked| locked.display_name())
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Records what the core emits.
    #[derive(Default)]
    pub struct Recorder {
        pub events: Mutex<Vec<AppEvent>>,
        pub fronts: Mutex<u32>,
    }

    impl Notifier for Arc<Recorder> {
        fn emit(&self, event: AppEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn bring_to_front(&self) {
            *self.fronts.lock().unwrap() += 1;
        }
    }

    pub fn core() -> (Arc<AppCore>, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let workspace = Workspace::new(Arc::new(keefast_workspace::LocalStorage));
        let core = AppCore::new(workspace, Box::new(recorder.clone()), Launch::default());
        (core, recorder)
    }

    #[test]
    fn every_update_tells_the_ui_with_a_higher_revision() {
        let (core, recorder) = core();

        core.update(|workspace| workspace.note_activity());
        core.update(|workspace| workspace.note_activity());

        let revisions: Vec<u32> = recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                AppEvent::WorkspaceChanged(changed) => Some(changed.revision),
                _ => None,
            })
            .collect();
        assert_eq!(revisions, [1, 2]);
        assert_eq!(core.revision(), 2);
    }

    #[test]
    fn reading_tells_the_ui_nothing() {
        let (core, recorder) = core();

        core.read(|workspace| workspace.vaults().count());

        assert!(recorder.events.lock().unwrap().is_empty());
    }

    #[test]
    fn a_new_pairing_request_comes_first_and_is_announced_once() {
        let none = Requests::default();
        let access = Requests {
            access: Some(0),
            ..none
        };
        let both = Requests {
            pairing: Some(3),
            ..access
        };

        assert_eq!(access.news_since(none), Some(BrowserRequestKind::Access));
        assert_eq!(access.news_since(access), None);
        assert_eq!(both.news_since(access), Some(BrowserRequestKind::Pairing));
        assert_eq!(access.news_since(both), Some(BrowserRequestKind::Access));
        assert_eq!(none.news_since(both), None);
    }
}
