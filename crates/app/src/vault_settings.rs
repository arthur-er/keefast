use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, PromptLevel,
    SharedString, Window, div, prelude::*,
};
use keefast_core::DefaultAccessPolicy;
use keefast_workspace::{VaultId, Workspace};

use crate::browser_integration::BrowserIntegration;
use crate::strings;
use crate::theme::Theme;
use crate::vault_browser::describe;
use crate::widgets::{button, tool_button};

/// Emitted when the user closes the settings.
pub struct VaultSettingsClosed;

const POLICIES: [(DefaultAccessPolicy, &str); 3] = [
    (DefaultAccessPolicy::Ask, strings::POLICY_ASK),
    (DefaultAccessPolicy::AlwaysAllow, strings::POLICY_ALLOW),
    (DefaultAccessPolicy::AlwaysDeny, strings::POLICY_DENY),
];

/// A Vault's settings, shown in the detail pane: its Default access policy and its Pairings,
/// which can be revoked. Tab moves between the choices, and Enter or Space picks one.
pub struct VaultSettings {
    workspace: Entity<Workspace>,
    integration: Entity<BrowserIntegration>,
    vault: VaultId,
    focus: FocusHandle,
    /// One per Default access policy, in [`POLICIES`] order.
    policy_focus: Vec<FocusHandle>,
    /// One per Pairing's Revoke button, in Vault file order.
    revoke_focus: Vec<FocusHandle>,
    error: Option<SharedString>,
}

impl EventEmitter<VaultSettingsClosed> for VaultSettings {}

impl Focusable for VaultSettings {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl VaultSettings {
    pub fn new(
        workspace: Entity<Workspace>,
        integration: Entity<BrowserIntegration>,
        vault: VaultId,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        let policy_focus = POLICIES
            .iter()
            .map(|_| cx.focus_handle().tab_stop(true))
            .collect();
        Self {
            workspace,
            integration,
            vault,
            focus: cx.focus_handle(),
            policy_focus,
            revoke_focus: Vec::new(),
            error: None,
        }
    }

    /// Where the focus starts: on the policy in force.
    /// The Vault whose settings are shown.
    pub fn vault(&self) -> VaultId {
        self.vault
    }

    pub fn initial_focus(&self, cx: &App) -> FocusHandle {
        let current = self.workspace.read(cx).default_access_policy(self.vault);
        POLICIES
            .iter()
            .position(|(policy, _)| Some(*policy) == current)
            .map_or_else(|| self.focus.clone(), |ix| self.policy_focus[ix].clone())
    }

    fn set_policy(&mut self, policy: DefaultAccessPolicy, cx: &mut Context<Self>) {
        let vault = self.vault;
        let result = self.integration.update(cx, |integration, cx| {
            integration.set_default_access_policy(vault, policy, cx)
        });
        self.error = result.err().map(|error| describe(&error).into());
        cx.notify();
    }

    /// Revokes the Pairing named `name`, once the user confirms.
    fn revoke(&mut self, name: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let question = strings::CONFIRM_REVOKE_PAIRING.replace("{name}", &name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(strings::CONFIRM_REVOKE_PAIRING_DETAIL),
            &[strings::REVOKE_PAIRING, strings::CANCEL],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            this.update(cx, |this, cx| {
                let vault = this.vault;
                let result = this.integration.update(cx, |integration, cx| {
                    integration.revoke_pairing(vault, &name, cx)
                });
                this.error = result.err().map(|error| describe(&error).into());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Escape closes the settings, and Enter or Space on a focused choice picks it.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => cx.emit(VaultSettingsClosed),
            "enter" | "space" => {
                if let Some(ix) = self
                    .policy_focus
                    .iter()
                    .position(|focus| focus.is_focused(window))
                {
                    self.set_policy(POLICIES[ix].0, cx);
                } else if let Some(ix) = self
                    .revoke_focus
                    .iter()
                    .position(|focus| focus.is_focused(window))
                {
                    let pairings = self.pairing_names(cx);
                    if let Some(name) = pairings.get(ix).cloned() {
                        self.revoke(name, window, cx);
                    }
                } else {
                    return;
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn pairing_names(&self, cx: &App) -> Vec<SharedString> {
        self.workspace
            .read(cx)
            .vault(self.vault)
            .map(|open| open.vault().pairings())
            .unwrap_or_default()
            .into_iter()
            .map(|pairing| pairing.name.into())
            .collect()
    }
}

impl Render for VaultSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let Some(open) = self.workspace.read(cx).vault(self.vault) else {
            return div().into_any_element();
        };
        let title = strings::VAULT_SETTINGS_TITLE.replace("{vault}", &open.display_name());
        let current = open.vault().default_access_policy();
        let pairings = self.pairing_names(cx);
        while self.revoke_focus.len() < pairings.len() {
            self.revoke_focus.push(cx.focus_handle().tab_stop(true));
        }
        let heading =
            |text: &'static str| div().font_weight(gpui::FontWeight::SEMIBOLD).child(text);
        let note = |text: &'static str| div().text_sm().text_color(theme.muted).child(text);

        let policies = POLICIES.iter().zip(&self.policy_focus).enumerate().map(
            |(ix, (&(policy, label), focus))| {
                let chosen = policy == current;
                div()
                    .id(("access-policy", ix))
                    .track_focus(focus)
                    .flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus(|style| style.border_color(theme.accent))
                    .cursor_pointer()
                    .child(if chosen {
                        strings::RADIO_ON
                    } else {
                        strings::RADIO_OFF
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_policy(policy, cx)))
            },
        );

        let pairing_rows =
            pairings
                .iter()
                .zip(&self.revoke_focus)
                .enumerate()
                .map(|(ix, (name, focus))| {
                    let name = name.clone();
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .py_1()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(div().truncate().child(name.clone()))
                        .child(
                            tool_button(("revoke-pairing", ix), strings::REVOKE_PAIRING, theme)
                                .track_focus(focus)
                                .focus(|style| style.border_color(theme.accent))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.revoke(name.clone(), window, cx)
                                })),
                        )
                });

        div()
            .id("vault-settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_lg().child(title))
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.error).child(error))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(heading(strings::DEFAULT_ACCESS_POLICY))
                    .child(note(strings::DEFAULT_ACCESS_POLICY_NOTE))
                    .children(policies),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(heading(strings::PAIRINGS))
                    .child(note(strings::PAIRINGS_NOTE))
                    .when(pairings.is_empty(), |this| {
                        this.child(note(strings::PAIRINGS_NONE))
                    })
                    .children(pairing_rows),
            )
            .child(
                div().flex().justify_end().child(
                    button("close-vault-settings", strings::EDITOR_DONE, theme)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(VaultSettingsClosed))),
                ),
            )
            .into_any_element()
    }
}
