//! What the backend tells the web UI unasked. The UI refetches what it shows on
//! `WorkspaceChanged`; the others carry news it shows once, such as a Merge notice.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::dto::VaultId;

/// The Workspace changed: an edit, a save or sync finishing, a Merge, a lock, a browser request.
/// `revision` goes up by one each time, so the UI can tell a stale answer from a fresh one.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChanged {
    pub revision: u32,
}

/// Changes made to a Vault file elsewhere were Merged into its Vault.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MergeNotice {
    pub vault: VaultId,
    pub vault_name: String,
    /// One line saying what changed, such as "Merged changes made elsewhere into Personal:
    /// 1 Entry updated".
    pub message: String,
}

/// A browser asks for something the user must answer: a Pairing, an Access decision, or where
/// a captured login goes. The request itself is in the workspace snapshot; keefast came to the
/// front.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRequest {
    pub kind: BrowserRequestKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BrowserRequestKind {
    Pairing,
    Access,
    Login,
}

/// What keefast last copied was cleared from the clipboard: its time ran out, or a Vault
/// locked. A notice about the copy is out of date.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardCleared {}

/// A save or sync of a Vault failed; the Vault stays unsaved, and its status says so.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SaveFailed {
    pub vault: VaultId,
    pub vault_name: String,
    pub message: String,
    pub detail: Option<String>,
}

/// Every event, for the notifier.
#[derive(Debug, Clone)]
pub enum AppEvent {
    WorkspaceChanged(WorkspaceChanged),
    MergeNotice(MergeNotice),
    BrowserRequest(BrowserRequest),
    ClipboardCleared(ClipboardCleared),
    SaveFailed(SaveFailed),
}

/// Where events go: the webview in the app, a recorder in tests.
pub trait Notifier: Send + Sync {
    fn emit(&self, event: AppEvent);

    /// Brings keefast's window to the front, as when a browser asks something: the user is in
    /// the browser at the time.
    fn bring_to_front(&self);
}
