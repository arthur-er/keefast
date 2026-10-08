use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, SharedString, Stateful, Window, div, prelude::*, px,
};
use keefast_workspace::{ThemeSetting, Workspace};

use crate::actions::{self, CloseSettings};
use crate::browser_integration::{self, BrowserIntegration};
use crate::password_generator::{PasswordGenerator, PasswordGeneratorEvent};
use crate::saving;
use crate::strings;
use crate::theme::{self, Theme};
use crate::widgets::button;

/// Emitted when the user is done with the settings.
pub struct SettingsClosed;

/// The waits offered for the idle lock and for clearing the clipboard; `None` is never.
const IDLE_LOCK_WAITS: [Option<u64>; 7] = [
    Some(60),
    Some(5 * 60),
    Some(15 * 60),
    Some(30 * 60),
    Some(60 * 60),
    Some(4 * 60 * 60),
    None,
];
const CLIPBOARD_WAITS: [Option<u64>; 7] = [
    Some(10),
    Some(20),
    Some(30),
    Some(60),
    Some(2 * 60),
    Some(5 * 60),
    None,
];

/// A setting one control changes.
#[derive(Clone, Copy)]
enum Change {
    Theme(ThemeSetting),
    IdleLock(Option<Duration>),
    LockOnOsLock,
    ClipboardClear(Option<Duration>),
    Autosave,
    BrowserIntegration,
    DownloadFavicons,
}

/// The settings screen: the app config's machine-local preferences, each changed and kept as
/// soon as it is picked. Every control is reached with Tab and picked with Enter or Space.
pub struct SettingsView {
    workspace: Entity<Workspace>,
    integration: Entity<BrowserIntegration>,
    generator: Entity<PasswordGenerator>,
    focus: FocusHandle,
    /// One per control, in the order they are drawn.
    controls: Vec<FocusHandle>,
}

impl EventEmitter<SettingsClosed> for SettingsView {}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsView {
    pub fn new(
        workspace: Entity<Workspace>,
        integration: Entity<BrowserIntegration>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        cx.observe(&integration, |_, _, cx| cx.notify()).detach();
        let generator = cx.new(|cx| PasswordGenerator::for_settings(workspace.clone(), cx));
        cx.subscribe(&generator, |_, _, event, cx| {
            if let PasswordGeneratorEvent::Cancel = event {
                cx.emit(SettingsClosed);
            }
        })
        .detach();
        Self {
            workspace,
            integration,
            generator,
            focus: cx.focus_handle(),
            controls: Vec::new(),
        }
    }

    fn change(&mut self, change: Change, cx: &mut Context<Self>) {
        match change {
            Change::Theme(setting) => {
                self.workspace
                    .update(cx, |workspace, _| workspace.set_theme(setting));
                theme::choose(setting, cx);
            }
            Change::IdleLock(after) => self
                .workspace
                .update(cx, |workspace, _| workspace.set_idle_lock_after(after)),
            Change::LockOnOsLock => self.workspace.update(cx, |workspace, _| {
                workspace.set_lock_on_os_lock(!workspace.lock_on_os_lock())
            }),
            Change::ClipboardClear(after) => self.workspace.update(cx, |workspace, _| {
                workspace.set_clipboard_clear_after(after)
            }),
            Change::Autosave => {
                self.workspace.update(cx, |workspace, _| {
                    workspace.set_autosave(!workspace.autosave())
                });
                saving::autosave(&self.workspace, cx);
            }
            Change::BrowserIntegration => self.integration.update(cx, |integration, cx| {
                integration.set_enabled(!integration.enabled(cx), cx)
            }),
            Change::DownloadFavicons => self.workspace.update(cx, |workspace, _| {
                workspace.set_download_favicons(!workspace.downloads_favicons())
            }),
        }
        // Those watching the Workspace, such as the favicon downloads, see the change.
        self.workspace.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Tab moves between the controls.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "tab" && !keystroke.modifiers.secondary() {
            if keystroke.modifiers.shift {
                window.focus_prev();
            } else {
                window.focus_next();
            }
            cx.stop_propagation();
        }
    }

    /// A control that makes `change` when clicked, or when focused and Enter or Space is
    /// pressed. `index` counts the controls drawn so far.
    fn control(
        &mut self,
        index: &mut usize,
        change: Change,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        while self.controls.len() <= *index {
            self.controls.push(cx.focus_handle().tab_stop(true));
        }
        let focus = &self.controls[*index];
        let id = *index;
        *index += 1;
        div()
            .id(("setting", id))
            .track_focus(focus)
            .cursor_pointer()
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .focus(|style| style.border_color(theme.accent))
            .on_click(cx.listener(move |this, _, _, cx| this.change(change, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.change(change, cx);
                    cx.stop_propagation();
                }
            }))
    }

    fn checkbox(
        &mut self,
        index: &mut usize,
        change: Change,
        on: bool,
        label: &'static str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.control(index, change, theme, cx)
            .flex()
            .gap_1()
            .px_1()
            .mx_neg_1()
            .child(if on {
                strings::CHECKBOX_ON
            } else {
                strings::CHECKBOX_OFF
            })
            .child(label)
            .into_any_element()
    }

    /// One button per choice, the chosen one highlighted.
    fn choices<T: PartialEq + Copy>(
        &mut self,
        index: &mut usize,
        choices: &[(T, SharedString)],
        chosen: T,
        change: impl Fn(T) -> Change,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let buttons: Vec<AnyElement> = choices
            .iter()
            .map(|(value, label)| {
                let selected = *value == chosen;
                self.control(index, change(*value), theme, cx)
                    .px_2()
                    .py_0p5()
                    .text_sm()
                    .border_color(theme.border)
                    .when(selected, |this| {
                        this.bg(theme.accent).text_color(theme.accent_text)
                    })
                    .child(label.clone())
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(buttons)
            .into_any_element()
    }
}

/// The waits to offer, with the one chosen among them even if it isn't one of `offered`, as
/// when set in the config file by hand.
fn waits(
    offered: &[Option<u64>],
    chosen: Option<Duration>,
) -> Vec<(Option<Duration>, SharedString)> {
    let mut waits: Vec<Option<Duration>> = offered
        .iter()
        .map(|seconds| seconds.map(Duration::from_secs))
        .collect();
    if !waits.contains(&chosen) {
        let at = waits
            .iter()
            .position(|wait| wait.is_none_or(|wait| Some(wait) > chosen))
            .unwrap_or(waits.len());
        waits.insert(at, chosen);
    }
    waits
        .into_iter()
        .map(|wait| (wait, describe_wait(wait).into()))
        .collect()
}

/// A wait as the settings show it: `30 s`, `5 min`, `1 h` or `Never`.
fn describe_wait(wait: Option<Duration>) -> String {
    let Some(wait) = wait else {
        return strings::NEVER.to_owned();
    };
    let seconds = wait.as_secs();
    let (template, n) = if seconds >= 60 * 60 && seconds % (60 * 60) == 0 {
        (strings::DURATION_HOURS, seconds / (60 * 60))
    } else if seconds >= 60 && seconds % 60 == 0 {
        (strings::DURATION_MINUTES, seconds / 60)
    } else {
        (strings::DURATION_SECONDS, seconds)
    };
    template.replace("{n}", &n.to_string())
}

fn section(title: &'static str, theme: Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .pb_4()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted)
                .child(title),
        )
}

fn label(text: &'static str) -> gpui::Div {
    div().text_sm().child(text)
}

fn note(text: impl Into<SharedString>, theme: Theme) -> gpui::Div {
    div().text_xs().text_color(theme.muted).child(text.into())
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let workspace = self.workspace.read(cx);
        let chosen_theme = workspace.theme();
        let idle_lock_after = workspace.idle_lock_after();
        let lock_on_os_lock = workspace.lock_on_os_lock();
        let clipboard_clear_after = workspace.clipboard_clear_after();
        let autosave = workspace.autosave();
        let browser_integration = workspace.browser_integration();
        let download_favicons = workspace.downloads_favicons();
        let config_error = workspace.config_error().is_some();
        let browsers = browser_integration::status(&self.integration, cx);

        let mut index = 0;
        let themes = [
            (ThemeSetting::System, strings::THEME_SYSTEM.into()),
            (ThemeSetting::Light, strings::THEME_LIGHT.into()),
            (ThemeSetting::Dark, strings::THEME_DARK.into()),
        ];
        let theme_choices =
            self.choices(&mut index, &themes, chosen_theme, Change::Theme, theme, cx);
        let idle_choices = self.choices(
            &mut index,
            &waits(&IDLE_LOCK_WAITS, idle_lock_after),
            idle_lock_after,
            Change::IdleLock,
            theme,
            cx,
        );
        let os_lock = self.checkbox(
            &mut index,
            Change::LockOnOsLock,
            lock_on_os_lock,
            strings::SETTINGS_LOCK_ON_OS_LOCK,
            theme,
            cx,
        );
        let clipboard_choices = self.choices(
            &mut index,
            &waits(&CLIPBOARD_WAITS, clipboard_clear_after),
            clipboard_clear_after,
            Change::ClipboardClear,
            theme,
            cx,
        );
        let autosave = self.checkbox(
            &mut index,
            Change::Autosave,
            autosave,
            strings::SETTINGS_AUTOSAVE,
            theme,
            cx,
        );
        let browser = self.checkbox(
            &mut index,
            Change::BrowserIntegration,
            browser_integration,
            strings::SETTINGS_BROWSER_INTEGRATION,
            theme,
            cx,
        );
        let favicons = self.checkbox(
            &mut index,
            Change::DownloadFavicons,
            download_favicons,
            strings::SETTINGS_FAVICONS,
            theme,
            cx,
        );

        div()
            .id("settings")
            .key_context(actions::SETTINGS_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(|_, _: &CloseSettings, _, cx| cx.emit(SettingsClosed)))
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .text_color(theme.text)
            .child(
                div()
                    .w(px(640.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_6()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_xl().child(strings::SETTINGS_TITLE))
                            .child(
                                button("close-settings", strings::SETTINGS_CLOSE, theme)
                                    .bg(theme.accent)
                                    .text_color(theme.accent_text)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsClosed))),
                            ),
                    )
                    .when(config_error, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(theme.error)
                                .child(strings::CONFIG_ERROR),
                        )
                    })
                    .child(
                        section(strings::SETTINGS_APPEARANCE, theme)
                            .child(label(strings::SETTINGS_THEME))
                            .child(theme_choices),
                    )
                    .child(
                        section(strings::SETTINGS_LOCKING, theme)
                            .child(label(strings::SETTINGS_IDLE_LOCK))
                            .child(idle_choices)
                            .child(os_lock),
                    )
                    .child(
                        section(strings::SETTINGS_CLIPBOARD, theme)
                            .child(label(strings::SETTINGS_CLIPBOARD_CLEAR))
                            .child(clipboard_choices),
                    )
                    .child(section(strings::SETTINGS_SAVING, theme).child(autosave))
                    .child(
                        section(strings::SETTINGS_GENERATOR, theme)
                            .child(note(strings::SETTINGS_GENERATOR_NOTE, theme))
                            .child(self.generator.clone()),
                    )
                    .child(
                        section(strings::SETTINGS_BROWSER, theme)
                            .child(browser)
                            .when_some(browsers, |this, (summary, failed)| {
                                this.child(
                                    note(summary, theme)
                                        .when(failed, |this| this.text_color(theme.error)),
                                )
                            }),
                    )
                    .child(
                        section(strings::SETTINGS_NETWORK, theme)
                            .border_b_0()
                            .child(favicons)
                            .child(note(strings::SETTINGS_FAVICONS_NOTE, theme)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_read_in_the_largest_whole_unit() {
        let describe = |seconds| describe_wait(Some(Duration::from_secs(seconds)));
        assert_eq!(describe(30), "30 s");
        assert_eq!(describe(90), "90 s");
        assert_eq!(describe(120), "2 min");
        assert_eq!(describe(15 * 60), "15 min");
        assert_eq!(describe(90 * 60), "90 min");
        assert_eq!(describe(4 * 60 * 60), "4 h");
        assert_eq!(describe_wait(None), "Never");
    }

    #[test]
    fn a_wait_set_by_hand_is_offered_in_its_place() {
        let offered = [Some(10), Some(60), None];
        let labels = |chosen: Option<u64>| -> Vec<String> {
            waits(&offered, chosen.map(Duration::from_secs))
                .into_iter()
                .map(|(_, label)| label.to_string())
                .collect()
        };
        assert_eq!(labels(Some(60)), ["10 s", "1 min", "Never"]);
        assert_eq!(labels(Some(45)), ["10 s", "45 s", "1 min", "Never"]);
        assert_eq!(labels(Some(3600)), ["10 s", "1 min", "1 h", "Never"]);
        assert_eq!(labels(Some(5)), ["5 s", "10 s", "1 min", "Never"]);
    }
}
