//! keefast's Card Plugin: payment cards kept on an Entry in plain custom fields (`Card number`,
//! `Cardholder`, `Expiry`, `CVV`, `PIN`), as `plugin.json` declares. Its hooks give the number's
//! label ("Visa •••• 1111"), format what the user types, and check it. It has no clock, so
//! whether a card has expired is for the host to say, from the `monthYear` Owned field.

pub mod card;

wit_bindgen::generate!({
    path: "../../crates/plugins/wit",
    world: "plugin",
});

use keefast::plugin::types::{FieldText, Severity};

/// The only facet this Plugin contributes.
const FACET: &str = "card";

struct Card;

impl Guest for Card {
    fn format(facet: String, key: String, value: String) -> String {
        if facet != FACET {
            return value;
        }
        card::format(&key, &value)
    }

    fn validate(facet: String, key: String, value: String) -> Option<Problem> {
        if facet != FACET {
            return None;
        }
        card::problem(&key, &value).map(|(error, message)| Problem {
            severity: if error {
                Severity::Error
            } else {
                Severity::Warning
            },
            message: message.to_owned(),
        })
    }

    fn derive(facet: String, fields: Vec<Field>) -> Derived {
        let number = fields
            .iter()
            .find(|field| field.key == "number")
            .and_then(|field| field.value.as_deref());
        let label = (facet == FACET)
            .then(|| number.and_then(card::label))
            .flatten();
        Derived {
            fields: label
                .iter()
                .map(|text| FieldText {
                    key: "number".to_owned(),
                    text: text.clone(),
                })
                .collect(),
            label,
        }
    }
}

export!(Card);
