//! Saving Vaults off the UI thread, for every view that edits them.

use gpui::{App, Entity};
use keefast_workspace::{PendingSave, Workspace};

/// Saves every Vault with unsaved changes.
pub fn save_all(workspace: &Entity<Workspace>, cx: &mut App) {
    let saves = workspace.update(cx, |workspace, _| workspace.begin_saves());
    run(workspace, saves, cx);
}

/// With autosave on, saves every Vault with unsaved changes. Call it after edits.
pub fn autosave(workspace: &Entity<Workspace>, cx: &mut App) {
    let saves = workspace.update(cx, |workspace, _| workspace.begin_autosaves());
    run(workspace, saves, cx);
}

/// Encrypts and writes off the UI thread; edits made meanwhile are autosaved afterwards.
fn run(workspace: &Entity<Workspace>, saves: Vec<PendingSave>, cx: &mut App) {
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
