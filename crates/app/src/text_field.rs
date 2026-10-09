use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};
use zeroize::Zeroizing;

use crate::actions;
use crate::icons::{self, UiIcon};
use crate::strings;
use crate::theme::Theme;

/// What a text field tells its owner.
pub enum TextFieldEvent {
    /// Enter in a single-line field.
    Submit,
    /// Escape.
    Cancel,
}

/// A minimal keyboard-driven text field: typing, a caret moved with the arrow keys, Home and
/// End, Backspace and Delete, paste, and Tab to the next field. Its text is wiped on drop, as
/// it may hold a password.
pub struct TextField {
    text: Zeroizing<String>,
    /// A byte offset into `text`, always on a character boundary.
    cursor: usize,
    focus: FocusHandle,
    placeholder: SharedString,
    multiline: bool,
    masked: bool,
    /// Drawn before the text, as a search field's magnifier.
    icon: Option<UiIcon>,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextField {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            text: empty_text(),
            cursor: 0,
            focus: cx.focus_handle().tab_stop(true),
            placeholder: placeholder.into(),
            multiline: false,
            masked: false,
            icon: None,
        }
    }

    pub fn with_icon(mut self, icon: UiIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text = empty_text();
        self.text.push_str(text);
        self.cursor = self.text.len();
        cx.notify();
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        self.masked = masked;
        cx.notify();
    }

    fn insert(&mut self, text: &str) {
        let text = if self.multiline {
            text.replace("\r\n", "\n")
        } else {
            text.replace(['\r', '\n'], "")
        };
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }

    fn previous_boundary(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |ch| self.cursor + ch.len_utf8())
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        match keystroke.key.as_str() {
            "enter" if self.multiline && !modifiers.secondary() => self.insert("\n"),
            "enter" => cx.emit(TextFieldEvent::Submit),
            "escape" => cx.emit(TextFieldEvent::Cancel),
            "tab" if modifiers.shift => window.focus_prev(),
            "tab" => window.focus_next(),
            "backspace" => {
                let start = self.previous_boundary();
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
            }
            "delete" => {
                let end = self.next_boundary();
                self.text.replace_range(self.cursor..end, "");
            }
            "left" => self.cursor = self.previous_boundary(),
            "right" => self.cursor = self.next_boundary(),
            "home" => self.cursor = 0,
            "end" => self.cursor = self.text.len(),
            "v" if modifiers.secondary() => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = Zeroizing::new(text);
                    self.insert(&text);
                }
            }
            _ if modifiers.control || modifiers.alt || modifiers.platform => return,
            _ => match &keystroke.key_char {
                Some(typed) => self.insert(typed),
                None => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// Reserves room up front so typing doesn't reallocate and leave copies of a password behind.
fn empty_text() -> Zeroizing<String> {
    Zeroizing::new(String::with_capacity(256))
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let focused = self.focus.is_focused(window);
        let shown = if self.masked {
            let mask = |text: &str| strings::PASSWORD_MASK_CHAR.repeat(text.chars().count());
            (
                mask(&self.text[..self.cursor]),
                mask(&self.text[self.cursor..]),
            )
        } else {
            (
                self.text[..self.cursor].to_owned(),
                self.text[self.cursor..].to_owned(),
            )
        };
        let content: SharedString = match (self.text.is_empty(), focused) {
            (true, false) => self.placeholder.clone(),
            (_, true) => format!("{}{}{}", shown.0, strings::CARET, shown.1).into(),
            (false, false) => format!("{}{}", shown.0, shown.1).into(),
        };

        div()
            .id("text-field")
            .key_context(actions::TEXT_FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .w_full()
            .px_2()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(if focused { theme.accent } else { theme.border })
            .bg(theme.background)
            .when(self.multiline, |this| this.min_h_20())
            .when(self.text.is_empty() && !focused, |this| {
                this.text_color(theme.muted)
            })
            .when_some(self.icon, |this, icon| {
                this.flex().items_center().gap_1p5().child(icons::ui_icon(
                    icon,
                    px(14.),
                    theme.muted,
                ))
            })
            .child(content)
    }
}
