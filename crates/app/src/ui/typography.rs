use gpui::{FontWeight, Styled, px, relative};

/// DESIGN.md's type scale. Each sets size and weight; colour is the caller's.
pub trait Typography: Styled + Sized {
    /// An Entry's title in the detail, and page titles: 20px semibold.
    fn text_title(self) -> Self {
        self.text_size(px(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .line_height(px(26.))
    }

    /// A dialog's title: 15px semibold.
    fn text_dialog_title(self) -> Self {
        self.text_size(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .line_height(relative(1.35))
    }

    /// A section's title: 13px semibold.
    fn text_heading(self) -> Self {
        self.text_size(px(13.)).font_weight(FontWeight::SEMIBOLD)
    }

    /// Body text and rows: 13px.
    fn text_body(self) -> Self {
        self.text_size(px(13.)).font_weight(FontWeight::NORMAL)
    }

    /// Body text that reads as a control's label: 13px medium.
    fn text_strong(self) -> Self {
        self.text_size(px(13.)).font_weight(FontWeight::MEDIUM)
    }

    /// Secondary text and labels: 12px.
    fn text_secondary(self) -> Self {
        self.text_size(px(12.)).font_weight(FontWeight::NORMAL)
    }

    /// Section heads in the sidebar, column heads and counts: 11px medium.
    fn text_micro(self) -> Self {
        self.text_size(px(11.)).font_weight(FontWeight::MEDIUM)
    }
}

impl<T: Styled> Typography for T {}
