use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use keefast_core::AccessDecision;
use keefast_workspace::{AccessAnswer, AccessId};

use crate::strings;
use crate::theme::Theme;
use crate::widgets::button;

pub struct AccessDialogEvent(pub AccessAnswer);

/// An Entry the site asks for, as the dialog lists it.
pub struct AskedEntry {
    pub title: SharedString,
    pub username: SharedString,
    pub vault: SharedString,
}

/// Asks the user whether a site may have Entries: Allow or Deny, remembered on the Entries if
/// the user wants. Escape denies; nothing on the keyboard allows, so a keystroke meant for
/// something else never gives a site access.
pub struct AccessDialog {
    access: AccessId,
    site: SharedString,
    entries: Vec<AskedEntry>,
    remember: bool,
    focus: FocusHandle,
}

impl EventEmitter<AccessDialogEvent> for AccessDialog {}

impl Focusable for AccessDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AccessDialog {
    pub fn new(
        access: AccessId,
        site: String,
        entries: Vec<AskedEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            access,
            site: site.into(),
            entries,
            remember: false,
            focus: cx.focus_handle(),
        }
    }

    /// The Access request this dialog answers.
    pub fn access(&self) -> AccessId {
        self.access
    }

    fn answer(&mut self, decision: AccessDecision, cx: &mut Context<Self>) {
        cx.emit(AccessDialogEvent(AccessAnswer {
            decision,
            remember: self.remember,
        }));
    }

    fn toggle_remember(&mut self, cx: &mut Context<Self>) {
        self.remember = !self.remember;
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        self.answer(AccessDecision::Deny, cx);
        cx.stop_propagation();
    }
}

impl Render for AccessDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let entries = self.entries.iter().map(|entry| {
            let title = if entry.title.is_empty() {
                strings::UNTITLED.into()
            } else {
                entry.title.clone()
            };
            div()
                .flex()
                .justify_between()
                .gap_3()
                .px_3()
                .py_1p5()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(div().truncate().child(title))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted)
                                .truncate()
                                .child(entry.username.clone()),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted)
                        .flex_none()
                        .child(entry.vault.clone()),
                )
        });
        div()
            .id("access-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .w(px(460.))
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .text_color(theme.text)
            .shadow_lg()
            .child(div().text_xl().child(strings::ACCESS_TITLE))
            .child(div().child(strings::ACCESS_PROMPT.replace("{site}", &self.site)))
            .child(
                div()
                    .id("access-entries")
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .children(entries),
            )
            .child(
                div()
                    .id("remember-access")
                    .flex()
                    .gap_1()
                    .text_sm()
                    .cursor_pointer()
                    .child(if self.remember {
                        strings::CHECKBOX_ON
                    } else {
                        strings::CHECKBOX_OFF
                    })
                    .child(strings::ACCESS_REMEMBER.replace("{site}", &self.site))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_remember(cx))),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(button("deny-access", strings::ACCESS_DENY, theme).on_click(
                        cx.listener(|this, _, _, cx| this.answer(AccessDecision::Deny, cx)),
                    ))
                    .child(
                        button("allow-access", strings::ACCESS_ALLOW, theme)
                            .bg(theme.accent)
                            .text_color(theme.accent_text)
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.answer(AccessDecision::Allow, cx)
                                }),
                            ),
                    ),
            )
    }
}
