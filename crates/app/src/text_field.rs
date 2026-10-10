use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, EventEmitter, FocusHandle,
    Focusable, HighlightStyle, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, SharedString, StyledText, TextLayout, Window, canvas, div, fill,
    point, prelude::*, px, size,
};
use zeroize::Zeroizing;

use crate::actions;
use crate::icons::{self, IconName};
use crate::strings;
use crate::theme::Theme;

/// What a text field tells its owner.
pub enum TextFieldEvent {
    /// Enter in a single-line field.
    Submit,
    /// Escape.
    Cancel,
}

/// A text field that behaves as the platform's do: a caret and a selection moved with the
/// arrow keys, Home and End, by word with Ctrl (Alt on macOS) and extended with Shift; select
/// all, copy, cut, paste, undo and redo; and a click to place the caret, a drag to select, a
/// double-click to select a word. Its text is wiped on drop, as it may hold a password, and a
/// masked field won't copy or cut it.
pub struct TextField {
    text: Zeroizing<String>,
    /// Where the selection began: a byte offset into `text`, always on a character boundary.
    anchor: usize,
    /// Where the caret is, and the selection ends: `anchor` when nothing is selected.
    head: usize,
    focus: FocusHandle,
    placeholder: SharedString,
    multiline: bool,
    masked: bool,
    /// Drawn before the text, as a search field's magnifier.
    icon: Option<IconName>,
    /// Drawn as a search box: sunk into the pane, with no edge until focused.
    search: bool,
    /// Drawn in the mono face, as secrets and codes are.
    mono: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// What the last change was, so a run of typing or deleting undoes as one.
    last_edit: Option<Edit>,
    /// The text as last laid out, and what was shown, to find where a click lands.
    layout: Option<(TextLayout, SharedString)>,
    /// Whether a drag with the mouse is selecting.
    selecting: bool,
}

/// The field as it was before a change, for undo and redo.
struct Snapshot {
    text: Zeroizing<String>,
    anchor: usize,
    head: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Edit {
    Typing,
    Deleting,
    Other,
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
            anchor: 0,
            head: 0,
            focus: cx.focus_handle().tab_stop(true),
            placeholder: placeholder.into(),
            multiline: false,
            masked: false,
            icon: None,
            search: false,
            mono: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            layout: None,
            selecting: false,
        }
    }

    pub fn with_icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Draws it as a search box.
    pub fn search(mut self) -> Self {
        self.search = true;
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

    /// Replaces the text, with the caret after it. What was typed before can't be undone.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text = empty_text();
        self.text.push_str(text);
        self.anchor = self.text.len();
        self.head = self.text.len();
        self.undo.clear();
        self.redo.clear();
        self.last_edit = None;
        cx.notify();
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        self.masked = masked;
        cx.notify();
    }

    /// Selects all the text, as a field just clicked into to change it does.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.anchor = 0;
        self.head = self.text.len();
        self.last_edit = None;
        cx.notify();
    }

    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// Moves the caret to `offset`, taking the selection with it when `extend`.
    fn move_to(&mut self, offset: usize, extend: bool) {
        if !extend {
            self.anchor = offset;
        }
        self.head = offset;
        self.last_edit = None;
    }

    /// Replaces the selection with `text`, as one undo step with the edits of the same kind
    /// just before it.
    fn replace_selection(&mut self, text: &str, edit: Edit) {
        let range = self.selection();
        if range.is_empty() && text.is_empty() {
            return;
        }
        let text = if self.multiline {
            Zeroizing::new(text.replace("\r\n", "\n"))
        } else {
            Zeroizing::new(text.replace(['\r', '\n'], ""))
        };
        if edit == Edit::Other || self.last_edit != Some(edit) {
            self.undo.push(self.snapshot());
            self.redo.clear();
        }
        self.text.replace_range(range.clone(), &text);
        self.anchor = range.start + text.len();
        self.head = self.anchor;
        self.last_edit = Some(edit);
    }

    fn snapshot(&self) -> Snapshot {
        let mut text = empty_text();
        text.push_str(&self.text);
        Snapshot {
            text,
            anchor: self.anchor,
            head: self.head,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = empty_text();
        self.text.push_str(&snapshot.text);
        self.anchor = snapshot.anchor;
        self.head = snapshot.head;
        self.last_edit = None;
    }

    fn undo(&mut self) {
        if let Some(snapshot) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(snapshot);
        }
    }

    fn redo(&mut self) {
        if let Some(snapshot) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(snapshot);
        }
    }

    /// Puts the selection on the clipboard, unless it's masked or empty.
    fn copy(&self, cx: &mut Context<Self>) -> bool {
        let selection = self.selection();
        if self.masked || selection.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.text[selection].to_owned()));
        true
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = Zeroizing::new(text);
            self.replace_selection(&text, Edit::Other);
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.text[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.text[offset..]
            .chars()
            .next()
            .map_or(offset, |ch| offset + ch.len_utf8())
    }

    /// The start of the word before `offset`. A masked field is one word, as its words
    /// would give away where its spaces are.
    fn previous_word_start(&self, offset: usize) -> usize {
        if self.masked {
            return 0;
        }
        let mut chars = self.text[..offset]
            .char_indices()
            .rev()
            .skip_while(|(_, ch)| ch.is_whitespace())
            .peekable();
        let Some(&(mut start, first)) = chars.peek() else {
            return 0;
        };
        for (index, ch) in chars {
            if char_kind(ch) != char_kind(first) {
                break;
            }
            start = index;
        }
        start
    }

    /// The end of the word after `offset`.
    fn next_word_end(&self, offset: usize) -> usize {
        if self.masked {
            return self.text.len();
        }
        let mut chars = self.text[offset..]
            .char_indices()
            .skip_while(|(_, ch)| ch.is_whitespace())
            .peekable();
        let Some(&(_, first)) = chars.peek() else {
            return self.text.len();
        };
        chars
            .find(|&(_, ch)| char_kind(ch) != char_kind(first))
            .map_or(self.text.len(), |(index, _)| offset + index)
    }

    /// The word, run of spaces or run of punctuation at `offset`, for a double-click.
    fn word_at(&self, offset: usize) -> Range<usize> {
        if self.masked {
            return 0..self.text.len();
        }
        let Some(kind) = self.text[offset..]
            .chars()
            .next()
            .or_else(|| self.text[..offset].chars().next_back())
            .map(char_kind)
        else {
            return offset..offset;
        };
        let start = self.text[..offset]
            .char_indices()
            .rev()
            .take_while(|&(_, ch)| char_kind(ch) == kind)
            .last()
            .map_or(offset, |(index, _)| index);
        let end = self.text[offset..]
            .char_indices()
            .find(|&(_, ch)| char_kind(ch) != kind)
            .map_or(self.text.len(), |(index, _)| offset + index);
        start..end
    }

    fn line_start(&self, offset: usize) -> usize {
        self.text[..offset].rfind('\n').map_or(0, |index| index + 1)
    }

    fn line_end(&self, offset: usize) -> usize {
        self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |index| offset + index)
    }

    /// Where `offset` into the text is in what's shown, which a mask makes longer.
    fn shown_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.text[..offset].chars().count() * strings::PASSWORD_MASK_CHAR.len()
        } else {
            offset
        }
    }

    fn text_offset(&self, offset: usize) -> usize {
        if self.masked {
            let chars = offset / strings::PASSWORD_MASK_CHAR.len();
            self.text
                .char_indices()
                .nth(chars)
                .map_or(self.text.len(), |(index, _)| index)
        } else {
            offset.min(self.text.len())
        }
    }

    /// The offset into the text nearest `position`, as last laid out.
    fn offset_at(&self, position: Point<Pixels>) -> Option<usize> {
        let (layout, shown) = self.layout.as_ref()?;
        if self.text.is_empty() {
            return Some(0);
        }
        let index = match layout.index_for_position(position) {
            // On a character: before or after it, whichever edge is nearer.
            Ok(index) => {
                let next = shown[index..]
                    .chars()
                    .next()
                    .map_or(index, |ch| index + ch.len_utf8());
                match (
                    layout.position_for_index(index),
                    layout.position_for_index(next),
                ) {
                    (Some(before), Some(after))
                        if before.y == after.y && position.x - before.x > after.x - position.x =>
                    {
                        next
                    }
                    _ => index,
                }
            }
            Err(index) => index,
        };
        Some(self.text_offset(index))
    }

    /// The offset a line above or below the caret, in a multi-line field.
    fn vertical_offset(&self, down: bool) -> Option<usize> {
        let (layout, _) = self.layout.as_ref()?;
        let caret = layout.position_for_index(self.shown_offset(self.head))?;
        let line_height = layout.line_height();
        let y = if down {
            caret.y + line_height * 1.5
        } else {
            caret.y - line_height * 0.5
        };
        self.offset_at(point(caret.x, y))
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(offset) = self.offset_at(event.position) else {
            return;
        };
        match event.click_count {
            0 | 1 => self.move_to(offset, event.modifiers.shift),
            2 => {
                let word = self.word_at(offset);
                self.move_to(word.start, false);
                self.move_to(word.end, true);
            }
            _ => self.select_all(cx),
        }
        self.selecting = true;
        cx.notify();
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        if let Some(offset) = self.offset_at(position)
            && offset != self.head
        {
            self.move_to(offset, true);
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let extend = modifiers.shift;
        // By word: Alt on macOS, as Cmd goes to the line's ends there; Ctrl elsewhere.
        let by_word = if cfg!(target_os = "macos") {
            modifiers.alt
        } else {
            modifiers.control
        };
        let selection = self.selection();
        match keystroke.key.as_str() {
            "enter" if self.multiline && !modifiers.secondary() => {
                self.replace_selection("\n", Edit::Typing)
            }
            "enter" => cx.emit(TextFieldEvent::Submit),
            "escape" => cx.emit(TextFieldEvent::Cancel),
            "tab" if modifiers.shift => window.focus_prev(cx),
            "tab" => window.focus_next(cx),
            "backspace" => {
                if selection.is_empty() {
                    self.anchor = if by_word {
                        self.previous_word_start(self.head)
                    } else {
                        self.previous_boundary(self.head)
                    };
                }
                self.replace_selection("", Edit::Deleting);
            }
            "delete" => {
                if selection.is_empty() {
                    self.anchor = if by_word {
                        self.next_word_end(self.head)
                    } else {
                        self.next_boundary(self.head)
                    };
                }
                self.replace_selection("", Edit::Deleting);
            }
            "left" if modifiers.platform => self.move_to(self.line_start(self.head), extend),
            "right" if modifiers.platform => self.move_to(self.line_end(self.head), extend),
            "left" if by_word => self.move_to(self.previous_word_start(self.head), extend),
            "right" if by_word => self.move_to(self.next_word_end(self.head), extend),
            "left" if !extend && !selection.is_empty() => self.move_to(selection.start, false),
            "right" if !extend && !selection.is_empty() => self.move_to(selection.end, false),
            "left" => self.move_to(self.previous_boundary(self.head), extend),
            "right" => self.move_to(self.next_boundary(self.head), extend),
            "up" | "down" if self.multiline && modifiers.platform => {
                let end = if keystroke.key == "up" {
                    0
                } else {
                    self.text.len()
                };
                self.move_to(end, extend)
            }
            "up" | "down" if self.multiline => {
                if let Some(offset) = self.vertical_offset(keystroke.key == "down") {
                    self.move_to(offset, extend)
                }
            }
            "home" if self.multiline && !modifiers.control => {
                self.move_to(self.line_start(self.head), extend)
            }
            "end" if self.multiline && !modifiers.control => {
                self.move_to(self.line_end(self.head), extend)
            }
            "home" => self.move_to(0, extend),
            "end" => self.move_to(self.text.len(), extend),
            "a" if modifiers.secondary() => self.select_all(cx),
            "c" if modifiers.secondary() => {
                self.copy(cx);
            }
            "x" if modifiers.secondary() => {
                if self.copy(cx) {
                    self.replace_selection("", Edit::Other);
                }
            }
            "v" if modifiers.secondary() => self.paste(cx),
            "z" if modifiers.secondary() && modifiers.shift => self.redo(),
            "z" if modifiers.secondary() => self.undo(),
            "y" if modifiers.control && !cfg!(target_os = "macos") => self.redo(),
            _ if modifiers.control || modifiers.alt || modifiers.platform => return,
            _ => match &keystroke.key_char {
                Some(typed) => self.replace_selection(typed, Edit::Typing),
                None => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// Words are runs of letters and digits, spaces, or other characters.
fn char_kind(ch: char) -> u8 {
    if ch.is_whitespace() {
        0
    } else if ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
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
        let shown: SharedString = if self.text.is_empty() {
            self.placeholder.clone()
        } else if self.masked {
            strings::PASSWORD_MASK_CHAR
                .repeat(self.text.chars().count())
                .into()
        } else {
            self.text.as_str().to_owned().into()
        };
        let selection = self.selection();
        let highlight = HighlightStyle {
            background_color: Some(theme.accent_bg.into()),
            ..HighlightStyle::default()
        };
        let selected = (focused && !selection.is_empty()).then(|| {
            (
                self.shown_offset(selection.start)..self.shown_offset(selection.end),
                highlight,
            )
        });
        let text = StyledText::new(shown.clone()).with_highlights(selected);
        let layout = text.layout().clone();
        self.layout = Some((layout.clone(), shown));
        let caret = (focused && selection.is_empty()).then(|| self.shown_offset(self.head));
        let field = cx.entity();

        // Drawn over the text once it's laid out: the caret, and the drag that selects,
        // followed even outside the field.
        let overlay = canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                if let Some(caret) = caret.and_then(|caret| layout.position_for_index(caret)) {
                    window.paint_quad(fill(
                        Bounds::new(caret, size(px(1.5), layout.line_height())),
                        theme.fg_0,
                    ));
                }
                let dragged = field.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && event.dragging() {
                        dragged.update(cx, |field, cx| field.drag_to(event.position, cx));
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        field.update(cx, |field, _| field.selecting = false);
                    }
                });
            },
        )
        .absolute()
        .size_full();

        div()
            .id("text-field")
            .key_context(actions::TEXT_FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .cursor(CursorStyle::IBeam)
            .w_full()
            .flex()
            .gap(px(8.))
            .px(px(10.))
            .rounded(px(6.))
            .border_1()
            .text_size(px(13.))
            .text_color(theme.fg_0)
            .map(|this| {
                if self.multiline {
                    this.items_start().py(px(7.)).min_h(px(72.))
                } else {
                    this.items_center()
                        .h(if self.search { px(30.) } else { px(32.) })
                }
            })
            .map(|this| match (focused, self.search) {
                (true, _) => this
                    .bg(theme.bg_1)
                    .border_color(theme.accent)
                    .shadow(theme.focus_ring()),
                (false, true) => this
                    .bg(theme.bg_hover)
                    .border_color(gpui::transparent_black()),
                (false, false) => this.bg(theme.bg_1).border_color(theme.border_strong),
            })
            .when(
                (self.masked || self.mono) && !self.text.is_empty(),
                |this| this.font_family(crate::theme::MONO_FONT),
            )
            .when(self.text.is_empty(), |this| this.text_color(theme.fg_2))
            .when_some(self.icon, |this, icon| {
                this.child(icons::ui_icon(icon, px(14.), theme.fg_2))
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .child(text)
                    .child(overlay),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, TestAppContext, VisualTestContext};

    use super::*;

    fn open_field<'a>(
        text: &str,
        masked: bool,
        cx: &'a mut TestAppContext,
    ) -> (Entity<TextField>, &'a mut VisualTestContext) {
        let (field, cx) = cx.add_window_view(|_, cx| {
            let mut field = TextField::new("", cx);
            field.set_text(text, cx);
            field.set_masked(masked, cx);
            field
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&field.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (field, cx)
    }

    fn text(field: &Entity<TextField>, cx: &mut VisualTestContext) -> String {
        field.read_with(cx, |field, _| field.text().to_owned())
    }

    fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
    }

    #[gpui::test]
    fn select_all_then_typing_replaces_the_text(cx: &mut TestAppContext) {
        let (field, cx) = open_field("old name", false, cx);
        cx.simulate_keystrokes("ctrl-a n e w");
        assert_eq!(text(&field, cx), "new");
    }

    #[gpui::test]
    fn shift_with_the_arrows_and_home_selects(cx: &mut TestAppContext) {
        let (field, cx) = open_field("abcdef", false, cx);
        cx.simulate_keystrokes("left shift-left shift-left x");
        assert_eq!(text(&field, cx), "abcxf");
        cx.simulate_keystrokes("shift-home backspace");
        assert_eq!(text(&field, cx), "f");
    }

    #[gpui::test]
    fn ctrl_moves_and_deletes_by_word(cx: &mut TestAppContext) {
        let (field, cx) = open_field("user@example.com is mine", false, cx);
        cx.simulate_keystrokes("ctrl-backspace");
        assert_eq!(text(&field, cx), "user@example.com is ");
        cx.simulate_keystrokes("ctrl-left ctrl-left ctrl-delete");
        // Punctuation is a word of its own, as in a browser's fields.
        assert_eq!(text(&field, cx), "user@example. is ");
        cx.simulate_keystrokes("home ctrl-right ctrl-shift-right x");
        assert_eq!(text(&field, cx), "userxexample. is ");
    }

    #[gpui::test]
    fn undo_takes_back_a_run_of_typing_and_redo_puts_it_back(cx: &mut TestAppContext) {
        let (field, cx) = open_field("a", false, cx);
        cx.simulate_keystrokes("b c ctrl-a ctrl-x");
        assert_eq!(text(&field, cx), "");
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(text(&field, cx), "abc");
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(text(&field, cx), "a");
        cx.simulate_keystrokes("ctrl-shift-z ctrl-y");
        assert_eq!(text(&field, cx), "");
    }

    #[gpui::test]
    fn copies_cuts_and_pastes_the_selection(cx: &mut TestAppContext) {
        let (field, cx) = open_field("hello world", false, cx);
        cx.simulate_keystrokes("shift-ctrl-left ctrl-c");
        assert_eq!(clipboard(cx).as_deref(), Some("world"));
        cx.simulate_keystrokes("ctrl-x home ctrl-v");
        assert_eq!(text(&field, cx), "worldhello ");
    }

    #[gpui::test]
    fn a_masked_field_wont_copy_or_cut(cx: &mut TestAppContext) {
        let (field, cx) = open_field("secret", true, cx);
        cx.simulate_keystrokes("ctrl-a ctrl-c ctrl-x");
        assert_eq!(clipboard(cx), None);
        assert_eq!(text(&field, cx), "secret");
    }
}
