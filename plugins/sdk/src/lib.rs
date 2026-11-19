//! Write a keefast Plugin in Rust.
//!
//! A Plugin teaches keefast a kind of data kept on Entries, in plain custom fields it declares as
//! its Owned fields. Its manifest (`plugin.json`) says what those are and how keefast shows them;
//! its code, compiled to a WebAssembly component, answers three hooks:
//!
//! - [`Plugin::format`]: a value as the user typed it, written as the field keeps it;
//! - [`Plugin::validate`]: what is wrong with a value, if anything;
//! - [`Plugin::derive`]: a short label for the facet on one Entry, and text to show in place of
//!   its fields' values.
//!
//! Implement [`Plugin`] for a type and [`export!`] it:
//!
//! ```
//! use keefast_plugin_sdk::{Derived, Fields, Input, Plugin, Problem};
//!
//! struct Iban;
//!
//! impl Plugin for Iban {
//!     fn format(input: &Input) -> String {
//!         input.value.to_uppercase()
//!     }
//!
//!     fn validate(input: &Input) -> Option<Problem> {
//!         (input.key == "iban" && input.value.len() < 15)
//!             .then(|| Problem::error("An IBAN has at least 15 characters."))
//!     }
//!
//!     fn derive(_facet: &str, fields: &Fields) -> Derived {
//!         Derived::new().label(fields.text("iban").map(|iban| format!("IBAN {}", &iban[..4])))
//!     }
//! }
//!
//! keefast_plugin_sdk::export!(Iban);
//! ```
//!
//! Hooks are plain Rust functions, so they are tested natively with `cargo test`; `keefast-plugin
//! build` compiles them to the component keefast runs. The guide, docs/plugins/README.md in
//! keefast's repository, covers the manifest, the Plugin capabilities, the sandbox and packaging.

use std::marker::PhantomData;

#[doc(hidden)]
// The host's `validate` takes its arguments flattened: eight of them.
#[allow(clippy::too_many_arguments)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../crates/plugins/wit",
        world: "plugin",
        pub_export_macro: true,
        export_macro_name: "__export_world",
        // So a Plugin's tests can compare what its hooks return.
        additional_derives: [PartialEq, Eq],
    });
}

pub use bindings::keefast::plugin::types::{Derived, Field, FieldText, Problem, Severity};

/// The host API version this SDK writes Plugins for: the manifest's `apiVersion`.
pub const API_VERSION: u32 = 1;

/// A Plugin's hooks. Each has a default that does nothing, so a Plugin implements only those it
/// needs. keefast calls them for its facets only, with a facet id from its manifest.
pub trait Plugin {
    /// A value as the user typed it into an Owned field, written as the field keeps it: digits
    /// in groups, a slash in a date. The default keeps it as typed.
    fn format(input: &Input) -> String {
        input.value.to_owned()
    }

    /// What is wrong with a value for an Owned field, if anything: an error keeps it from being
    /// saved, a warning doesn't. `input.today` is the month it is now, since a Plugin has no
    /// clock: say when a date has passed or is near by it. The default finds nothing wrong.
    fn validate(input: &Input) -> Option<Problem> {
        let _ = input;
        None
    }

    /// What to show for a facet on one Entry: a short label, such as "Visa •••• 1111", and text
    /// to show in place of fields' values. A protected field's value is only given if the
    /// manifest asks for the `readProtected` Plugin capability, and a label is shown without a
    /// reveal, so it must never be a whole protected value. The default shows nothing.
    fn derive(facet: &str, fields: &Fields) -> Derived {
        let _ = (facet, fields);
        Derived::new()
    }
}

/// A value the user typed into one of a facet's Owned fields, for [`Plugin::format`] and
/// [`Plugin::validate`]. Later API versions may tell a hook more, so it is built with
/// [`Input::new`] rather than as a struct literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Input<'a> {
    /// The facet's id in the manifest.
    pub facet: &'a str,
    /// The Owned field's `key` in the manifest.
    pub key: &'a str,
    pub value: &'a str,
    /// The month it is now, in UTC, which keefast gives [`Plugin::validate`] (a Plugin has no
    /// clock); `None` in [`Plugin::format`].
    pub today: Option<MonthYear>,
}

impl<'a> Input<'a> {
    pub fn new(facet: &'a str, key: &'a str, value: &'a str) -> Self {
        Self {
            facet,
            key,
            value,
            today: None,
        }
    }

    /// The same input, on `today`: as [`Plugin::validate`] gets it, for tests.
    pub fn with_today(mut self, today: MonthYear) -> Self {
        self.today = Some(today);
        self
    }
}

/// A facet's Owned fields on one Entry, by their `key` in the manifest, for [`Plugin::derive`].
#[derive(Debug, Clone, Default)]
pub struct Fields(Vec<Field>);

impl Fields {
    /// Fields as the host gives them.
    pub fn new(fields: Vec<Field>) -> Self {
        Self(fields)
    }

    /// Fields from keys and values, for tests: `Fields::of([("number", Some("4111…"))])`.
    pub fn of<'a>(fields: impl IntoIterator<Item = (&'a str, Option<&'a str>)>) -> Self {
        Self(
            fields
                .into_iter()
                .map(|(key, value)| Field {
                    key: key.to_owned(),
                    value: value.map(str::to_owned),
                })
                .collect(),
        )
    }

    /// A field's value. `None` when the Entry hasn't the field, when it is empty, or when it is
    /// protected and the Plugin hasn't the `readProtected` Plugin capability.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|field| field.key == key)
            .and_then(|field| field.value.as_deref())
            .filter(|value| !value.is_empty())
    }

    /// The digits of a `digits` field, without the spaces or dashes `format` put in.
    pub fn digits(&self, key: &str) -> Option<String> {
        let digits = digits(self.text(key)?);
        (!digits.is_empty()).then_some(digits)
    }

    /// A `monthYear` field, kept as `MM/YY`; `None` if it isn't one.
    pub fn month_year(&self, key: &str) -> Option<MonthYear> {
        MonthYear::parse(self.text(key)?)
    }

    /// Every field, in the manifest's order.
    pub fn iter(&self) -> impl Iterator<Item = &Field> {
        self.0.iter()
    }
}

/// A month of a year, as a `monthYear` Owned field keeps it (`MM/YY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonthYear {
    /// Four digits, such as 2026.
    pub year: u16,
    /// 1 to 12.
    pub month: u8,
}

impl MonthYear {
    /// `MM/YY`, the way a `monthYear` field is kept, read as a month of 2000 to 2099.
    pub fn parse(text: &str) -> Option<Self> {
        let (month, year) = text.trim().split_once('/')?;
        let all_digits = |part: &str| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
        if !all_digits(month) || !all_digits(year) {
            return None;
        }
        let month: u8 = month.parse().ok()?;
        let year: u16 = year.parse().ok()?;
        (1..=12).contains(&month).then_some(Self {
            year: 2000 + year,
            month,
        })
    }
}

impl std::fmt::Display for MonthYear {
    /// As a `monthYear` field keeps it: `MM/YY`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}/{:02}", self.month, self.year % 100)
    }
}

/// The ASCII digits of `text`, ignoring anything else.
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

impl Problem {
    /// The value can't be kept as it is. `message` is shown as is: one sentence saying what to do.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
        }
    }

    /// The value is probably a mistake, but can still be kept.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

impl Derived {
    /// Nothing to show: no label, and every field as it is.
    pub fn new() -> Self {
        Self {
            label: None,
            fields: Vec::new(),
        }
    }

    /// The facet's label on this Entry. keefast shows one line of at most 64 characters.
    pub fn label(mut self, label: impl Into<Option<String>>) -> Self {
        self.label = label.into();
        self
    }

    /// Text shown in place of a field's value, such as "Visa •••• 1111" for a protected number.
    pub fn text(mut self, key: impl Into<String>, text: impl Into<String>) -> Self {
        self.fields.push(FieldText {
            key: key.into(),
            text: text.into(),
        });
        self
    }
}

impl Default for Derived {
    fn default() -> Self {
        Self::new()
    }
}

/// Turns a [`Plugin`] into the host API's exports; used by [`export!`].
#[doc(hidden)]
pub struct Exports<P>(PhantomData<P>);

impl<P: Plugin> bindings::Guest for Exports<P> {
    fn format(facet: String, key: String, value: String) -> String {
        P::format(&Input::new(&facet, &key, &value))
    }

    fn validate(
        facet: String,
        key: String,
        value: String,
        today: bindings::keefast::plugin::types::MonthYear,
    ) -> Option<Problem> {
        let today = MonthYear {
            year: today.year,
            month: today.month,
        };
        P::validate(&Input::new(&facet, &key, &value).with_today(today))
    }

    fn derive(facet: String, fields: Vec<Field>) -> Derived {
        P::derive(&facet, &Fields::new(fields))
    }
}

/// Exports a type implementing [`Plugin`] as the Plugin's hooks. Call it once, in the crate built
/// to the component (`crate-type = ["cdylib"]`).
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        #[doc(hidden)]
        type __KeefastPluginExports = $crate::Exports<$plugin>;
        $crate::bindings::__export_world!(__KeefastPluginExports with_types_in $crate::bindings);
    };
}

#[cfg(test)]
mod tests {
    use super::bindings::Guest;
    use super::*;

    struct Shouting;

    impl Plugin for Shouting {
        fn format(input: &Input) -> String {
            input.value.to_uppercase()
        }

        fn validate(input: &Input) -> Option<Problem> {
            let today = input.today?;
            (today.year < 2027).then(|| Problem::warning(format!("Today is {today}.")))
        }

        fn derive(facet: &str, fields: &Fields) -> Derived {
            Derived::new()
                .label(fields.text("name").map(|name| format!("{facet}: {name}")))
                .text("name", "hidden")
        }
    }

    #[test]
    fn hooks_reach_the_plugin() {
        type Hooks = Exports<Shouting>;
        assert_eq!(Hooks::format("f".into(), "k".into(), "abc".into()), "ABC");
        let month = |year, month| bindings::keefast::plugin::types::MonthYear { year, month };
        assert_eq!(
            Hooks::validate("f".into(), "k".into(), "abc".into(), month(2026, 10)),
            Some(Problem::warning("Today is 10/26."))
        );
        assert_eq!(
            Hooks::validate("f".into(), "k".into(), "abc".into(), month(2027, 1)),
            None
        );
        let fields = vec![Field {
            key: "name".into(),
            value: Some("Ada".into()),
        }];
        let derived = Hooks::derive("person".into(), fields);
        assert_eq!(derived.label.as_deref(), Some("person: Ada"));
        assert_eq!(derived.fields[0].text, "hidden");
    }

    #[test]
    fn fields_are_read_by_key_and_kind() {
        let fields = Fields::of([
            ("number", Some("4111 1111-1111 1111")),
            ("expiry", Some("11/26")),
            ("empty", Some("")),
            ("pin", None),
        ]);
        assert_eq!(fields.digits("number").as_deref(), Some("4111111111111111"));
        assert_eq!(
            fields.month_year("expiry"),
            Some(MonthYear {
                year: 2026,
                month: 11
            })
        );
        assert_eq!(fields.text("empty"), None);
        assert_eq!(fields.text("pin"), None);
        assert_eq!(fields.text("missing"), None);
    }

    #[test]
    fn a_month_year_is_mm_slash_yy() {
        assert_eq!(MonthYear::parse("13/26"), None);
        assert_eq!(MonthYear::parse("1/26"), None);
        assert_eq!(MonthYear::parse("00/26"), None);
        assert_eq!(MonthYear::parse(" 03/07 ").unwrap().to_string(), "03/07");
    }

    #[test]
    fn problems_say_how_bad() {
        assert_eq!(Problem::error("No.").severity, Severity::Error);
        assert_eq!(Problem::warning("Hm.").severity, Severity::Warning);
    }
}
