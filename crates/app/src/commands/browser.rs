//! Answering the browsers' requests that wait for the user: Pairing, Access and captured
//! logins. The requests are in the workspace snapshot; each answer names the one it answers.

use keefast_core::AccessDecision;
use keefast_workspace::AccessAnswer;

use super::Core;
use super::workspace::{LoginTarget, RequestId};
use crate::browser;
use crate::error::{ApiError, ApiResult};

/// Pairs the extension that asked with its Vault, as `name`. A name the Vault can't take is
/// refused, and the request stays pending.
#[tauri::command]
#[specta::specta]
pub async fn approve_pairing(core: Core<'_>, id: RequestId, name: String) -> ApiResult<()> {
    let reply = core.edit(|workspace| {
        let Some(pending) = workspace
            .pending_pairing()
            .filter(|p| id.matches(p.id.as_u64()))
        else {
            return Ok(None);
        };
        workspace.approve_pairing(pending.id, &name)
    })?;
    browser::send(&core, reply);
    Ok(())
}

/// Turns the Pairing request down.
#[tauri::command]
#[specta::specta]
pub async fn deny_pairing(core: Core<'_>, id: RequestId) -> ApiResult<()> {
    let reply = core.update(|workspace| {
        let pending = workspace
            .pending_pairing()
            .filter(|p| id.matches(p.id.as_u64()))?;
        workspace.deny_pairing(pending.id)
    });
    browser::send(&core, reply);
    Ok(())
}

/// Answers the site that asked with the user's decision, remembered on the Entries for the site
/// if `remember`.
#[tauri::command]
#[specta::specta]
pub async fn answer_access(
    core: Core<'_>,
    id: RequestId,
    allow: bool,
    remember: bool,
) -> ApiResult<()> {
    let answer = AccessAnswer {
        decision: match allow {
            true => AccessDecision::Allow,
            false => AccessDecision::Deny,
        },
        remember,
    };
    let replies = core.update(|workspace| {
        match workspace
            .pending_access()
            .filter(|p| id.matches(p.id.as_u64()))
        {
            Some(pending) => workspace.answer_access(pending.id, answer),
            None => Vec::new(),
        }
    });
    browser::send(&core, replies);
    if remember {
        core.autosave();
    }
    Ok(())
}

/// Saves the captured login where the user chose. A place it can't go is refused, and the login
/// stays pending.
#[tauri::command]
#[specta::specta]
pub async fn save_login(core: Core<'_>, id: RequestId, target: LoginTarget) -> ApiResult<()> {
    let reply = core.update(|workspace| {
        let Some(pending) = workspace
            .pending_login()
            .filter(|p| id.matches(p.id.as_u64()))
        else {
            return Err(ApiError::not_found());
        };
        let target = target.to_workspace(workspace)?;
        Ok(workspace.save_login(pending.id, target)?)
    })?;
    browser::send(&core, reply);
    core.autosave();
    Ok(())
}

/// Turns the captured login down: nothing is saved.
#[tauri::command]
#[specta::specta]
pub async fn cancel_login(core: Core<'_>, id: RequestId) -> ApiResult<()> {
    let reply = core.update(|workspace| {
        let pending = workspace
            .pending_login()
            .filter(|p| id.matches(p.id.as_u64()))?;
        workspace.cancel_login(pending.id)
    });
    browser::send(&core, reply);
    Ok(())
}
