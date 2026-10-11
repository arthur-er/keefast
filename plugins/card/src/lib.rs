//! keefast's Card Plugin: payment cards kept on an Entry in plain custom fields (`Card number`,
//! `Cardholder`, `Expiry`, `CVV`, `PIN`), as `plugin.json` declares. Its hooks give the number's
//! label ("Visa •••• 1111"), format what the user types, and check it. It has no clock, so
//! whether a card has expired is for the host to say, from the `monthYear` Owned field.
//!
//! It is written with keefast's Plugin SDK, as a third-party Plugin is.

pub mod card;

use keefast_plugin_sdk::{Derived, Fields, Input, Plugin, Problem};

/// The only facet this Plugin contributes.
const FACET: &str = "card";

struct Card;

impl Plugin for Card {
    fn format(input: &Input) -> String {
        if input.facet != FACET {
            return input.value.to_owned();
        }
        card::format(input.key, input.value)
    }

    fn validate(input: &Input) -> Option<Problem> {
        if input.facet != FACET {
            return None;
        }
        card::problem(input.key, input.value).map(|(error, message)| {
            if error {
                Problem::error(message)
            } else {
                Problem::warning(message)
            }
        })
    }

    fn derive(facet: &str, fields: &Fields) -> Derived {
        let label = (facet == FACET)
            .then(|| fields.text("number").and_then(card::label))
            .flatten();
        match label {
            Some(label) => Derived::new().text("number", label.clone()).label(label),
            None => Derived::new(),
        }
    }
}

keefast_plugin_sdk::export!(Card);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_number_is_labelled_by_brand_and_last_digits() {
        let fields = Fields::of([("number", Some("4111 1111 1111 1111")), ("cvv", None)]);
        let derived = Card::derive(FACET, &fields);
        assert_eq!(derived.label.as_deref(), Some("Visa •••• 1111"));
        assert_eq!(derived.fields[0].key, "number");
        assert_eq!(Card::derive("other", &fields), Derived::new());
        assert_eq!(Card::derive(FACET, &Fields::of([])), Derived::new());
    }

    #[test]
    fn hooks_answer_only_for_the_card_facet() {
        assert_eq!(
            Card::format(&Input::new(FACET, "number", "4111111111111111")),
            "4111 1111 1111 1111"
        );
        assert_eq!(
            Card::format(&Input::new("other", "number", "4111111111111111")),
            "4111111111111111"
        );
        assert!(Card::validate(&Input::new(FACET, "number", "4111111111111112")).is_some());
        assert!(Card::validate(&Input::new("other", "number", "1")).is_none());
    }
}
