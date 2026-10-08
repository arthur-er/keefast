//! Saving Vaults off the UI thread, for every view that edits them, and Merging in changes
//! made to their Vault files elsewhere.

use std::time::Duration;

use gpui::{App, Entity};
use keefast_workspace::{PendingSave, VaultId, Workspace};

/// How long a Vault file must stay unchanged before it is read: sync tools write in steps.
const SETTLE: Duration = Duration::from_millis(500);

/// Checks Dropbox Vaults for changes made elsewhere whenever one is due, Merging them in and
/// uploading saves made offline, for as long as the app runs.
pub fn sync_online_vaults(workspace: &Entity<Workspace>, cx: &mut App) {
    let workspace = workspace.clone();
    cx.spawn(async move |cx| {
        // With no check due, look again soon: a Vault may be opened or unlocked meanwhile,
        // with a save made offline to upload at once.
        const AT_MOST: Duration = Duration::from_secs(5);
        while let Ok(wait) = workspace.read_with(cx, |workspace, _| workspace.sync_in()) {
            cx.background_executor()
                .timer(wait.unwrap_or(AT_MOST).min(AT_MOST))
                .await;
            let synced = cx.update(|cx| {
                let syncs = workspace.update(cx, |workspace, _| workspace.begin_syncs());
                run(&workspace, syncs, cx);
            });
            if synced.is_err() {
                return;
            }
        }
    })
    .detach();
}

/// Checks every Dropbox Vault now, without waiting for its turn.
pub fn sync_now(workspace: &Entity<Workspace>, cx: &mut App) {
    let syncs = workspace.update(cx, |workspace, _| workspace.sync_now());
    run(workspace, syncs, cx);
}

/// Watches the open Vaults' Vault files, and Merges in changes made elsewhere once each has
/// settled, saving the Merge.
pub fn watch_vault_files(workspace: &Entity<Workspace>, cx: &mut App) {
    let (changes, receiver) = async_channel::unbounded();
    workspace.update(cx, |workspace, _| {
        workspace.watch_vault_files(move |vault| {
            let _ = changes.send_blocking(vault);
        })
    });
    let workspace = workspace.clone();
    cx.spawn(async move |cx| {
        while let Ok(first) = receiver.recv().await {
            let mut changed = vec![first];
            loop {
                cx.background_executor().timer(SETTLE).await;
                let more: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
                if more.is_empty() {
                    break;
                }
                changed.extend(more);
            }
            let updated = cx.update(|cx| {
                workspace.update(cx, |workspace, _| {
                    for vault in changed {
                        workspace.vault_file_changed(vault);
                    }
                });
                autosave(&workspace, cx);
            });
            if updated.is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Saves every Vault with unsaved changes.
pub fn save_all(workspace: &Entity<Workspace>, cx: &mut App) {
    let saves = workspace.update(cx, |workspace, _| workspace.begin_saves());
    run(workspace, saves, cx);
}

/// Saves one Vault now, unless it is being saved already.
pub fn save(workspace: &Entity<Workspace>, vault: VaultId, cx: &mut App) {
    let save = workspace.update(cx, |workspace, _| workspace.begin_save(vault).ok());
    run(workspace, save.into_iter().collect(), cx);
}

/// With autosave on, saves every Vault with unsaved changes, and Merges in changes made to
/// Vault files elsewhere. Call it after edits.
pub fn autosave(workspace: &Entity<Workspace>, cx: &mut App) {
    let saves = workspace.update(cx, |workspace, _| workspace.begin_autosaves());
    run(workspace, saves, cx);
}

/// Encrypts and writes off the UI thread; edits made meanwhile are autosaved afterwards.
pub fn run(workspace: &Entity<Workspace>, saves: Vec<PendingSave>, cx: &mut App) {
    for save in saves {
        let saving = cx.background_executor().spawn(async move { save.run() });
        let workspace = workspace.clone();
        cx.spawn(async move |cx| {
            let finished = saving.await;
            cx.update(|cx| {
                // A failure shows in the save status; the Vault stays unsaved.
                let _ = workspace.update(cx, |workspace, cx| {
                    cx.notify();
                    workspace.finish_save(finished)
                });
                autosave(&workspace, cx);
            })
            .ok();
        })
        .detach();
    }
    workspace.update(cx, |_, cx| cx.notify());
}
