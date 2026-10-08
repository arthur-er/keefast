use gpui::{
    Action, AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, KeyDownEvent, KeybindingKeystroke, MouseButton, ScrollHandle, SharedString,
    StyledText, Window, div, prelude::*, px,
};
use keefast_core::fuzzy_match;

use crate::actions::{self, SelectNextCommand, SelectPreviousCommand};
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;

/// Emitted when the palette closes, after running a command or not.
pub struct Dismissed;

/// Runs any action the view that had focus handles, picked by typing part of its name. Up and
/// Down pick among the matches, Enter runs one and Escape closes the palette.
pub struct CommandPalette {
    focus: FocusHandle,
    query: Entity<TextField>,
    /// What had focus before the palette, where its commands are run.
    previous: Option<FocusHandle>,
    commands: Vec<Command>,
    matches: Vec<Match>,
    selected: usize,
    scroll: ScrollHandle,
    /// The query the matches are for.
    matched_query: Option<String>,
    dismissed: bool,
}

struct Command {
    action: Box<dyn Action>,
    name: SharedString,
    keys: Option<SharedString>,
}

struct Match {
    command: usize,
    /// The byte offsets of the name's characters the query matched.
    positions: Vec<usize>,
}

impl EventEmitter<Dismissed> for CommandPalette {}

impl Focusable for CommandPalette {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl CommandPalette {
    /// Opens over what has focus now, listing the actions it handles, and takes the focus.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let previous = window.focused(cx);
        let mut commands: Vec<Command> = window
            .available_actions(cx)
            .into_iter()
            .filter(|action| !action.name().starts_with(PALETTE_NAMESPACE))
            .map(|action| {
                let keys = previous
                    .as_ref()
                    .and_then(|previous| {
                        window
                            .bindings_for_action_in(action.as_ref(), previous)
                            .pop()
                    })
                    .map(|binding| keys_label(binding.keystrokes()).into());
                Command {
                    name: command_name(action.name()).into(),
                    action,
                    keys,
                }
            })
            .collect();
        commands.sort_by(|a, b| a.name.cmp(&b.name));

        let query = cx.new(|cx| TextField::new(strings::PALETTE_PLACEHOLDER, cx));
        cx.subscribe_in(&query, window, Self::on_query_event)
            .detach();
        cx.observe(&query, |this, _, cx| this.filter(cx)).detach();
        let focus = cx.focus_handle();
        // Something else took the focus, such as a browser request's dialog. Switching to
        // another window doesn't count: the palette is still there to come back to.
        cx.on_focus_out(&focus, window, |this, _, window, cx| {
            if window.is_window_active() {
                this.close(cx);
            }
        })
        .detach();
        window.focus(&query.focus_handle(cx));

        let mut palette = Self {
            focus,
            query,
            previous,
            commands,
            matches: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            matched_query: None,
            dismissed: false,
        };
        palette.filter(cx);
        palette
    }

    /// Closes the palette and gives the focus back to what had it before.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous) = &self.previous {
            window.focus(previous);
        }
        self.close(cx);
    }

    /// Closes the palette and leaves the focus where it is, for when something else took it.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if !self.dismissed {
            self.dismissed = true;
            cx.emit(Dismissed);
        }
    }

    /// Ranks the commands against the query: the best match first, and every command, by name,
    /// for an empty query. Keys that don't change the query, such as Left, keep the pick.
    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).text();
        if self.matched_query.as_deref() == Some(query) {
            return;
        }
        self.matched_query = Some(query.to_owned());
        let mut ranked: Vec<(i32, Match)> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(command, listed)| {
                let found = fuzzy_match(query, &listed.name)?;
                Some((
                    found.score,
                    Match {
                        command,
                        positions: found.positions,
                    },
                ))
            })
            .collect();
        // Stable, so equal scores stay in name order.
        ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        self.matches = ranked.into_iter().map(|(_, found)| found).collect();
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNextCommand, _: &mut Window, cx: &mut Context<Self>) {
        if !self.matches.is_empty() {
            self.select((self.selected + 1) % self.matches.len(), cx);
        }
    }

    fn select_previous(
        &mut self,
        _: &SelectPreviousCommand,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.matches.len();
        if len > 0 {
            self.select((self.selected + len - 1) % len, cx);
        }
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = ix;
        self.scroll.scroll_to_item(ix);
        cx.notify();
    }

    fn on_query_event(
        &mut self,
        _: &Entity<TextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit => self.run(self.selected, window, cx),
            TextFieldEvent::Cancel => self.dismiss(window, cx),
        }
    }

    /// Closes the palette and runs a matched command on what had focus before it.
    fn run(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(found) = self.matches.get(ix) else {
            return;
        };
        let action = self.commands[found.command].action.boxed_clone();
        self.dismiss(window, cx);
        window.dispatch_action(action, cx);
    }

    fn render_command(
        &self,
        ix: usize,
        found: &Match,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let command = &self.commands[found.command];
        let matched = HighlightStyle {
            color: Some(theme.accent.into()),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let highlights = found.positions.iter().map(|&start| {
            let len = command.name[start..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
            (start..start + len, matched)
        });
        div()
            .id(("command", ix))
            .flex()
            .gap_4()
            .px_3()
            .py_1p5()
            .cursor_pointer()
            .when(ix == self.selected, |this| this.bg(theme.selection))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .child(StyledText::new(command.name.clone()).with_highlights(highlights)),
            )
            .when_some(command.keys.clone(), |this, keys| {
                this.child(
                    div()
                        .flex_none()
                        .text_sm()
                        .text_color(theme.muted)
                        .child(keys),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.run(ix, window, cx)))
            .into_any_element()
    }
}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let rows: Vec<AnyElement> = self
            .matches
            .iter()
            .enumerate()
            .map(|(ix, found)| self.render_command(ix, found, theme, cx))
            .collect();
        div()
            .id("command-palette")
            .key_context(actions::COMMAND_PALETTE_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            // Clicks on the palette don't reach what is behind it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // The query is its only field, so Tab doesn't take the focus out of the palette.
            .capture_key_down(|event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "tab" {
                    cx.stop_propagation();
                }
            })
            .w(px(480.))
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .text_color(theme.text)
            .shadow_lg()
            .child(
                div()
                    .p_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(self.query.clone()),
            )
            .child(
                div()
                    .id("command-list")
                    .max_h(px(320.))
                    .py_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .when(rows.is_empty(), |this| {
                        this.px_3()
                            .py_1p5()
                            .text_color(theme.muted)
                            .child(strings::PALETTE_NO_MATCHES)
                    })
                    .children(rows),
            )
    }
}

/// The palette's own actions, which it doesn't list.
const PALETTE_NAMESPACE: &str = "command_palette::";

/// A command's name: the one `strings` gives its action, or else one made from the action's,
/// so that `vault_browser::CopyUsername` is "Copy Username".
fn command_name(action_name: &str) -> String {
    if let Some(name) = strings::command_name(action_name) {
        return name.to_owned();
    }
    let name = action_name.rsplit("::").next().unwrap_or(action_name);
    let mut words: Vec<String> = Vec::new();
    let mut previous: Option<char> = None;
    for ch in name.chars() {
        let starts_word = match previous {
            None => true,
            Some(previous) => ch.is_uppercase() && !previous.is_uppercase(),
        };
        if ch == '_' {
            previous = None;
            continue;
        }
        match words.last_mut() {
            Some(word) if !starts_word => word.push(ch),
            _ => words.push(ch.to_string()),
        }
        previous = Some(ch);
    }
    words.join(" ")
}

/// How the keys of a binding are written: `Ctrl+Shift+P`, `Down`.
fn keys_label(keystrokes: &[KeybindingKeystroke]) -> String {
    keystrokes
        .iter()
        .map(|keystroke| {
            let modifiers = keystroke.modifiers();
            let mut parts: Vec<String> = Vec::new();
            let named = [
                (modifiers.control, strings::KEY_CTRL),
                (modifiers.alt, strings::KEY_ALT),
                (modifiers.platform, strings::KEY_SUPER),
                (modifiers.shift, strings::KEY_SHIFT),
            ];
            parts.extend(
                named
                    .into_iter()
                    .filter(|(held, _)| *held)
                    .map(|(_, name)| name.to_owned()),
            );
            let mut key = keystroke.key().chars();
            parts.push(match key.next() {
                Some(first) => first.to_uppercase().chain(key).collect(),
                None => String::new(),
            });
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use gpui::{KeyBinding, TestAppContext, VisualTestContext, actions};

    use super::*;
    use crate::actions::ToggleCommandPalette;

    #[test]
    fn names_commands_after_their_actions() {
        assert_eq!(
            command_name("test_view::SelectNextEntry"),
            "Select Next Entry"
        );
        assert_eq!(command_name("test_view::SaveAll"), "Save All");
        assert_eq!(command_name("Save"), "Save");
    }

    #[test]
    fn names_from_strings_come_first() {
        assert_eq!(command_name("vault_browser::CopyTotp"), "Copy TOTP Code");
        assert_eq!(command_name("vault_browser::OpenEntry"), "Edit Entry");
        assert_eq!(
            command_name("vault_browser::CopyCustomField"),
            "Copy Custom Field"
        );
    }

    #[test]
    fn writes_keys_out() {
        let keys =
            |keys: &str| keys_label(KeyBinding::new(keys, ToggleCommandPalette, None).keystrokes());
        assert_eq!(keys("ctrl-shift-p"), "Ctrl+Shift+P");
        assert_eq!(keys("down"), "Down");
        assert_eq!(keys("ctrl-k ctrl-s"), "Ctrl+K Ctrl+S");
    }

    actions!(test_view, [SaveAll, CopyPassword, CopyUsername]);

    /// A view with a few actions, which hosts the palette as the main window does.
    struct TestView {
        focus: FocusHandle,
        palette: Option<Entity<CommandPalette>>,
        ran: Vec<&'static str>,
    }

    impl TestView {
        fn toggle_palette(
            &mut self,
            _: &ToggleCommandPalette,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            if let Some(palette) = self.palette.take() {
                palette.update(cx, |palette, cx| palette.dismiss(window, cx));
                return;
            }
            let palette = cx.new(|cx| CommandPalette::new(window, cx));
            cx.subscribe(&palette, |this, _, _: &Dismissed, cx| {
                this.palette = None;
                cx.notify();
            })
            .detach();
            self.palette = Some(palette);
            cx.notify();
        }
    }

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .on_action(cx.listener(Self::toggle_palette))
                .child(
                    div()
                        .key_context("TestView")
                        .track_focus(&self.focus)
                        .on_action(cx.listener(|this, _: &SaveAll, _, _| this.ran.push("save")))
                        .on_action(
                            cx.listener(|this, _: &CopyPassword, _, _| this.ran.push("password")),
                        )
                        .on_action(
                            cx.listener(|this, _: &CopyUsername, _, _| this.ran.push("username")),
                        ),
                )
                .children(self.palette.clone())
        }
    }

    fn open_view(cx: &mut TestAppContext) -> (Entity<TestView>, &mut VisualTestContext) {
        cx.update(|cx| {
            actions::bind_keys(cx);
            cx.bind_keys([KeyBinding::new("ctrl-s", SaveAll, Some("TestView"))]);
        });
        let (view, cx) = cx.add_window_view(|_, cx| TestView {
            focus: cx.focus_handle(),
            palette: None,
            ran: Vec::new(),
        });
        cx.update(|window, cx| {
            // Focus changes are reported only in the active window.
            window.activate_window();
            window.focus(&view.read(cx).focus);
        });
        cx.run_until_parked();
        (view, cx)
    }

    fn shown(view: &Entity<TestView>, cx: &mut VisualTestContext) -> Vec<(String, Option<String>)> {
        cx.read(|cx| {
            let palette = view
                .read(cx)
                .palette
                .as_ref()
                .expect("the palette is open")
                .read(cx);
            palette
                .matches
                .iter()
                .map(|found| {
                    let command = &palette.commands[found.command];
                    (
                        command.name.to_string(),
                        command.keys.as_ref().map(ToString::to_string),
                    )
                })
                .collect()
        })
    }

    fn selected(view: &Entity<TestView>, cx: &mut VisualTestContext) -> String {
        cx.read(|cx| {
            let palette = view
                .read(cx)
                .palette
                .as_ref()
                .expect("the palette is open")
                .read(cx);
            palette.commands[palette.matches[palette.selected].command]
                .name
                .to_string()
        })
    }

    fn view_focused(view: &Entity<TestView>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| view.read(cx).focus.is_focused(window))
    }

    #[gpui::test]
    fn lists_every_action_the_focused_view_handles_with_its_keys(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p");
        assert_eq!(
            shown(&view, cx),
            vec![
                ("Copy Password".to_owned(), None),
                ("Copy Username".to_owned(), None),
                ("Save All".to_owned(), Some("Ctrl+S".to_owned())),
            ]
        );
    }

    #[gpui::test]
    fn typing_narrows_the_commands_and_enter_runs_the_best_match(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p c p");
        // "Copy Username" has a "p" too, but not at the start of a word.
        assert_eq!(
            shown(&view, cx)
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            vec!["Copy Password", "Copy Username"]
        );
        cx.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(cx, |view, _| view.ran.clone()),
            vec!["password"]
        );
        assert!(view.read_with(cx, |view, _| view.palette.is_none()));
        assert!(view_focused(&view, cx));
    }

    #[gpui::test]
    fn up_and_down_pick_the_command_to_run(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p");
        assert_eq!(selected(&view, cx), "Copy Password");
        cx.simulate_keystrokes("down down");
        assert_eq!(selected(&view, cx), "Save All");
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&view, cx), "Copy Password");
        cx.simulate_keystrokes("up");
        assert_eq!(selected(&view, cx), "Save All");
        cx.simulate_keystrokes("enter");
        assert_eq!(view.read_with(cx, |view, _| view.ran.clone()), vec!["save"]);
    }

    #[gpui::test]
    fn moving_the_caret_keeps_the_pick(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p c o p y down left home");
        assert_eq!(selected(&view, cx), "Copy Username");
    }

    #[gpui::test]
    fn tab_stays_in_the_palette(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p tab shift-tab s a v e enter");
        assert_eq!(view.read_with(cx, |view, _| view.ran.clone()), vec!["save"]);
    }

    #[gpui::test]
    fn escape_closes_without_running_anything(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p s a v e escape");
        assert!(view.read_with(cx, |view, _| view.palette.is_none() && view.ran.is_empty()));
        assert!(view_focused(&view, cx));
        // Its keys act on the view again.
        cx.simulate_keystrokes("ctrl-s");
        assert_eq!(view.read_with(cx, |view, _| view.ran.clone()), vec!["save"]);
    }

    #[gpui::test]
    fn the_same_keys_close_it(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p");
        assert!(view.read_with(cx, |view, _| view.palette.is_some()));
        cx.simulate_keystrokes("ctrl-shift-p");
        assert!(view.read_with(cx, |view, _| view.palette.is_none()));
        assert!(view_focused(&view, cx));
    }

    #[gpui::test]
    fn enter_with_nothing_matching_does_nothing(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p x x x");
        assert!(shown(&view, cx).is_empty());
        cx.simulate_keystrokes("enter");
        assert!(view.read_with(cx, |view, _| view.palette.is_some() && view.ran.is_empty()));
    }

    #[gpui::test]
    fn stays_open_while_another_window_is_active(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p");
        cx.deactivate_window();
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.palette.is_some()));
    }

    #[gpui::test]
    fn closes_when_something_else_takes_the_focus(cx: &mut TestAppContext) {
        let (view, cx) = open_view(cx);
        cx.simulate_keystrokes("ctrl-shift-p");
        cx.update(|window, cx| window.focus(&view.read(cx).focus));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.palette.is_none()));
    }
}
