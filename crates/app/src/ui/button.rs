use gpui::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, RenderOnce, SharedString, StatefulInteractiveElement,
    Styled, Window, div, prelude::FluentBuilder, px,
};

use crate::icons::{IconName, ui_icon};
use crate::theme::Theme;

use super::Kbd;

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The one primary action of a view, on the accent.
    Primary,
    /// Raised, with an edge.
    #[default]
    Secondary,
    /// Text only, until hovered.
    Ghost,
    /// Destroys something, as a ghost button in the danger colour.
    DangerGhost,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonSize {
    /// 32px.
    #[default]
    Medium,
    /// 26px, for toolbars and rows.
    Small,
}

impl ButtonSize {
    fn height(self) -> Pixels {
        match self {
            Self::Medium => px(32.),
            Self::Small => px(26.),
        }
    }
}

/// A button with a label, and an icon or key caps beside it if given.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    kbd: Option<Kbd>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    focus: Option<FocusHandle>,
    on_click: Option<OnClick>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: Some(label.into()),
            icon: None,
            kbd: None,
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            disabled: false,
            focus: None,
            on_click: None,
        }
    }

    /// Reachable with Tab, ringed when it has the focus.
    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn danger_ghost(self) -> Self {
        self.variant(ButtonVariant::DangerGhost)
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn small(mut self) -> Self {
        self.size = ButtonSize::Small;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Key caps after the label, for the shortcut that does the same.
    pub fn kbd(mut self, kbd: Option<Kbd>) -> Self {
        self.kbd = kbd;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let small = self.size == ButtonSize::Small;
        let on_accent = self.variant == ButtonVariant::Primary;
        let icon_color = match self.variant {
            ButtonVariant::Primary => theme.fg_on_accent,
            ButtonVariant::DangerGhost => theme.danger,
            _ => theme.fg_1,
        };
        let content: Vec<AnyElement> = self
            .icon
            .map(|icon| ui_icon(icon, px(14.), icon_color).into_any_element())
            .into_iter()
            .chain(self.label.map(|label| label.into_any_element()))
            .chain(
                self.kbd
                    .map(|kbd| if on_accent { kbd.on_accent() } else { kbd }.into_any_element()),
            )
            .collect();
        frame(self.id, self.variant, false, theme)
            .h(self.size.height())
            .px(if small { px(8.) } else { px(12.) })
            .gap(px(6.))
            .rounded(if small { px(5.) } else { px(6.) })
            .text_size(if small { px(12.) } else { px(13.) })
            .children(content)
            .when(self.disabled, |this| this.opacity(0.45))
            .when_some(self.focus, |this, focus| {
                this.track_focus(&focus)
                    .focus(|style| style.shadow(theme.focus_ring()))
            })
            .when_some(self.on_click.filter(|_| !self.disabled), |this, handler| {
                this.on_click(handler)
            })
    }
}

/// A square button showing only an icon.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: ButtonSize,
    selected: bool,
    tooltip: Option<SharedString>,
    on_click: Option<OnClick>,
}

impl IconButton {
    /// A ghost icon button, as toolbars have.
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            size: ButtonSize::Medium,
            selected: false,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn small(mut self) -> Self {
        self.size = ButtonSize::Small;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Shown when hovered, saying what the icon does.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let size = self.size.height();
        let color = if self.selected {
            theme.fg_0
        } else {
            theme.fg_1
        };
        frame(self.id, ButtonVariant::Ghost, self.selected, theme)
            .size(size)
            .rounded(px(6.))
            .child(ui_icon(self.icon, px(16.), color))
            .when_some(self.tooltip, |this, tooltip| {
                this.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
            })
            .when_some(self.on_click, |this, handler| this.on_click(handler))
    }
}

/// The box every button is drawn in, coloured for `variant`.
fn frame(
    id: ElementId,
    variant: ButtonVariant,
    selected: bool,
    theme: Theme,
) -> gpui::Stateful<gpui::Div> {
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(gpui::transparent_black())
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .cursor_pointer();
    match variant {
        ButtonVariant::Primary => base
            .bg(theme.accent)
            .text_color(theme.fg_on_accent)
            .hover(|style| style.bg(theme.accent_hover)),
        ButtonVariant::Secondary => base
            .bg(if selected { theme.bg_2 } else { theme.bg_1 })
            .border_color(theme.border_strong)
            .text_color(theme.fg_0)
            .hover(|style| style.bg(theme.bg_2)),
        ButtonVariant::Ghost => base
            .when(selected, |this| this.bg(theme.bg_active))
            .text_color(if selected { theme.fg_0 } else { theme.fg_1 })
            .hover(|style| style.bg(theme.bg_hover).text_color(theme.fg_0)),
        ButtonVariant::DangerGhost => base
            .text_color(theme.danger)
            .hover(|style| style.bg(theme.danger_bg)),
    }
}
