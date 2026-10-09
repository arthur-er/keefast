use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, ScrollHandle, SharedString, Stateful, Window, WindowAppearance, div, prelude::*,
    px,
};
use keefast_workspace::{ThemeSetting, VaultId, Workspace};

use crate::actions::{self, CloseSettings};
use crate::browser_integration::{self, BrowserIntegration};
use crate::icons::{self, IconName};
use crate::password_generator::{PasswordGenerator, PasswordGeneratorEvent};
use crate::saving;
use crate::strings;
use crate::theme::{self, Theme};
use crate::vault_browser::{sidebar_row, sidebar_separator};
use crate::vault_settings::{VaultSettings, VaultSettingsChangeKey, VaultSettingsClosed};

/// Emitted when the user is done with the settings.
pub struct SettingsClosed;

/// Emitted when the user asks to change a Vault's master key, which the Vault browser asks for.
pub struct SettingsChangeVaultKey(pub VaultId);

/// What the settings screen shows: the app's settings, or one Vault's.
enum Page {
    General,
    Vault(Entity<VaultSettings>),
}

/// The sections of the app's settings, in the order they are drawn, each listed under General.
const SECTIONS: [&str; 7] = [
    strings::SETTINGS_APPEARANCE,
    strings::SETTINGS_LOCKING,
    strings::SETTINGS_CLIPBOARD,
    strings::SETTINGS_SAVING,
    strings::SETTINGS_GENERATOR,
    strings::SETTINGS_BROWSER,
    strings::SETTINGS_NETWORK,
];

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

/// The settings screen, KeeWeb's way: a menu of the app's settings and the open Vaults beside
/// the page picked. The app's settings are the app config's machine-local preferences, each
/// changed and kept as soon as it is picked. Every control is reached with Tab and picked with
/// Enter or Space.
pub struct SettingsView {
    workspace: Entity<Workspace>,
    integration: Entity<BrowserIntegration>,
    generator: Entity<PasswordGenerator>,
    focus: FocusHandle,
    /// One per control, in the order they are drawn.
    controls: Vec<FocusHandle>,
    page: Page,
    /// Scrolls the page, and to a section picked in the menu.
    scroll: ScrollHandle,
}

impl EventEmitter<SettingsClosed> for SettingsView {}
impl EventEmitter<SettingsChangeVaultKey> for SettingsView {}

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
        cx.observe(&workspace, |this, workspace, cx| {
            // A Vault's page goes when it locks.
            if let Page::Vault(settings) = &this.page {
                let vault = settings.read(cx).vault();
                if workspace.read(cx).vault(vault).is_none() {
                    this.page = Page::General;
                }
            }
            cx.notify()
        })
        .detach();
        cx.observe(&integration, |_, _, cx| cx.notify()).detach();
        let generator = cx.new(|cx| PasswordGenerator::for_settings(workspace.clone(), cx));
        cx.subscribe(&generator, |_, _, event, cx| {
            // Escape in the separator field.
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
            page: Page::General,
            scroll: ScrollHandle::new(),
        }
    }

    /// Shows `vault`'s settings.
    pub fn show_vault(&mut self, vault: VaultId, window: &mut Window, cx: &mut Context<Self>) {
        let (workspace, integration) = (self.workspace.clone(), self.integration.clone());
        let settings = cx.new(|cx| VaultSettings::new(workspace, integration, vault, cx));
        cx.subscribe(&settings, |_, _, _: &VaultSettingsClosed, cx| {
            cx.emit(SettingsClosed)
        })
        .detach();
        cx.subscribe(&settings, move |_, _, _: &VaultSettingsChangeKey, cx| {
            cx.emit(SettingsChangeVaultKey(vault))
        })
        .detach();
        window.focus(&settings.read(cx).initial_focus(cx), cx);
        self.page = Page::Vault(settings);
        self.scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    /// Shows the app's settings, at `section`.
    fn show_section(&mut self, section: usize, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.page, Page::Vault(_)) {
            self.page = Page::General;
            window.focus(&self.focus, cx);
        }
        // The page's header comes before the sections.
        self.scroll.scroll_to_top_of_item(section + 1);
        cx.notify();
    }

    /// The menu beside the page: the app's settings and their sections, then the open Vaults.
    fn render_menu(&self, theme: Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.read(cx);
        let shown_vault = match &self.page {
            Page::General => None,
            Page::Vault(settings) => Some(settings.read(cx).vault()),
        };
        let icon =
            |icon: IconName| Some(icons::ui_icon(icon, px(14.), theme.text).into_any_element());
        let sections = SECTIONS.iter().enumerate().map(|(ix, &title)| {
            sidebar_row(
                ("settings-section", ix),
                title.into(),
                1,
                false,
                None,
                theme,
            )
            .on_click(cx.listener(move |this, _, window, cx| this.show_section(ix, window, cx)))
        });
        let vaults = workspace.vaults().enumerate().map(|(ix, open)| {
            let vault = open.id();
            sidebar_row(
                ("settings-vault", ix),
                open.display_name().into(),
                0,
                shown_vault == Some(vault),
                icon(IconName::LockOpen),
                theme,
            )
            .on_click(cx.listener(move |this, _, window, cx| this.show_vault(vault, window, cx)))
        });
        div()
            .id("settings-menu")
            .w(px(220.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .py_1p5()
            .overflow_y_scroll()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                sidebar_row(
                    ("settings-general", 0),
                    strings::SETTINGS_GENERAL.into(),
                    0,
                    shown_vault.is_none(),
                    icon(IconName::Settings),
                    theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.show_section(0, window, cx))),
            )
            .children(sections)
            .child(sidebar_separator(theme))
            .children(vaults)
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
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
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

    /// A preview of `setting`, KeeWeb's way: a card in its background and text colours with a
    /// button in its action colour.
    fn theme_card(
        &mut self,
        index: &mut usize,
        setting: ThemeSetting,
        chosen: bool,
        appearance: WindowAppearance,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preview = Theme::of(setting, appearance);
        self.control(index, Change::Theme(setting), theme, cx)
            .w(px(140.))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .pt_1p5()
            .pb_3()
            .border_2()
            .border_color(if chosen { theme.accent } else { preview.border })
            .bg(preview.background)
            .text_color(preview.text)
            .text_sm()
            .child(
                div()
                    .w(px(110.))
                    .pb_1()
                    .border_b_1()
                    .border_color(preview.border)
                    .text_center()
                    .truncate()
                    .child(theme_name(setting)),
            )
            .child(
                div()
                    .w(px(84.))
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .bg(preview.accent)
                    .text_color(preview.accent_text)
                    .child(strings::THEME_PREVIEW_BUTTON),
            )
            .into_any_element()
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

fn theme_name(setting: ThemeSetting) -> &'static str {
    match setting {
        ThemeSetting::System => strings::THEME_SYSTEM,
        ThemeSetting::Dark => strings::THEME_DARK,
        ThemeSetting::Light => strings::THEME_LIGHT,
        ThemeSetting::SolarizedDark => strings::THEME_SOLARIZED_DARK,
        ThemeSetting::SolarizedLight => strings::THEME_SOLARIZED_LIGHT,
        ThemeSetting::DarkBlue => strings::THEME_DARK_BLUE,
        ThemeSetting::LightBlue => strings::THEME_LIGHT_BLUE,
        ThemeSetting::DarkBrown => strings::THEME_DARK_BROWN,
        ThemeSetting::LightBrown => strings::THEME_LIGHT_BROWN,
        ThemeSetting::Terminal => strings::THEME_TERMINAL,
        ThemeSetting::TerminalLight => strings::THEME_TERMINAL_LIGHT,
        ThemeSetting::DarkContrast => strings::THEME_DARK_CONTRAST,
        ThemeSetting::HighContrast => strings::THEME_HIGH_CONTRAST,
    }
}

fn section(title: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .pb_6()
        .child(div().text_xl().font_weight(FontWeight::BOLD).child(title))
}

/// The title of a settings page, with the way back to the Vaults.
fn page_header(
    icon: AnyElement,
    title: impl Into<SharedString>,
    theme: Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .pb_4()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .child(icon)
                .child(
                    div()
                        .text_3xl()
                        .font_weight(FontWeight::BOLD)
                        .truncate()
                        .child(title.into()),
                ),
        )
        .child(
            div()
                .id("close-settings")
                .flex_none()
                .flex()
                .items_center()
                .gap_1p5()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .hover(|this| this.bg(theme.sidebar_selection))
                .child(strings::SETTINGS_CLOSE)
                .child(icons::ui_icon(
                    IconName::CircleArrowLeft,
                    px(16.),
                    theme.text,
                ))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsClosed))),
        )
        .into_any_element()
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
        let appearance = window.appearance();
        let theme_cards: Vec<AnyElement> = ThemeSetting::ALL
            .into_iter()
            .map(|setting| {
                let chosen = setting == chosen_theme;
                self.theme_card(&mut index, setting, chosen, appearance, theme, cx)
            })
            .collect();
        let theme_choices = div().flex().flex_wrap().gap_2().children(theme_cards);
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

        let page = match &self.page {
            Page::Vault(settings) => {
                let name = self
                    .workspace
                    .read(cx)
                    .vault(settings.read(cx).vault())
                    .map(|open| open.display_name())
                    .unwrap_or_default();
                vec![
                    page_header(
                        icons::ui_icon(IconName::LockOpen, px(28.), theme.text).into_any_element(),
                        name,
                        theme,
                        cx,
                    ),
                    settings.clone().into_any_element(),
                ]
            }
            Page::General => vec![
                page_header(
                    icons::ui_icon(IconName::Settings, px(28.), theme.text).into_any_element(),
                    strings::SETTINGS,
                    theme,
                    cx,
                ),
                section(strings::SETTINGS_APPEARANCE)
                    .child(label(strings::SETTINGS_THEME))
                    .child(theme_choices)
                    .into_any_element(),
                section(strings::SETTINGS_LOCKING)
                    .child(label(strings::SETTINGS_IDLE_LOCK))
                    .child(idle_choices)
                    .child(os_lock)
                    .into_any_element(),
                section(strings::SETTINGS_CLIPBOARD)
                    .child(label(strings::SETTINGS_CLIPBOARD_CLEAR))
                    .child(clipboard_choices)
                    .into_any_element(),
                section(strings::SETTINGS_SAVING)
                    .child(autosave)
                    .when(config_error, |this| {
                        this.child(note(strings::CONFIG_ERROR, theme).text_color(theme.error))
                    })
                    .into_any_element(),
                section(strings::SETTINGS_GENERATOR)
                    .child(note(strings::SETTINGS_GENERATOR_NOTE, theme))
                    .child(self.generator.clone())
                    .into_any_element(),
                section(strings::SETTINGS_BROWSER)
                    .child(browser)
                    .when_some(browsers, |this, (summary, failed)| {
                        this.child(
                            note(summary, theme).when(failed, |this| this.text_color(theme.error)),
                        )
                    })
                    .into_any_element(),
                section(strings::SETTINGS_NETWORK)
                    .child(favicons)
                    .child(note(strings::SETTINGS_FAVICONS_NOTE, theme))
                    .into_any_element(),
            ],
        };

        div()
            .id("settings")
            .key_context(actions::SETTINGS_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(|_, _: &CloseSettings, _, cx| cx.emit(SettingsClosed)))
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.text)
            .child(self.render_menu(theme, cx))
            .child(
                div()
                    .id("settings-page")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px_8()
                    .py_6()
                    // The header spans the page, its sections keep to a readable width.
                    .children(page.into_iter().enumerate().map(|(ix, part)| {
                        div().when(ix > 0, |this| this.max_w(px(760.))).child(part)
                    })),
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
