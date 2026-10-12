//! A keefast Plugin for bank accounts, kept on an Entry in plain custom fields (`IBAN`, `BIC`,
//! `Account holder`), as `plugin.json` declares: the template to start a Plugin from.
//!
//! Copy this directory, then change the crate's name, the manifest's id, name, publisher and
//! facets, and the hooks below. docs/plugins/README.md in keefast's repository is the guide.

use keefast_plugin_sdk::{Derived, Fields, Input, Plugin, Problem};

/// The facet's id and its Owned fields' keys, as `plugin.json` has them.
const FACET: &str = "bank-account";
const IBAN: &str = "iban";
const BIC: &str = "bic";

struct BankAccount;

impl Plugin for BankAccount {
    /// An IBAN in groups of four, as printed on statements; a BIC in capitals.
    fn format(input: &Input) -> String {
        match (input.facet, input.key) {
            (FACET, IBAN) => compact(input.value)
                .as_bytes()
                .chunks(4)
                .map(|group| String::from_utf8_lossy(group).into_owned())
                .collect::<Vec<_>>()
                .join(" "),
            (FACET, BIC) => compact(input.value),
            _ => input.value.to_owned(),
        }
    }

    fn validate(input: &Input) -> Option<Problem> {
        match (input.facet, input.key) {
            (FACET, IBAN) if !input.value.trim().is_empty() => iban_problem(&compact(input.value)),
            (FACET, BIC) if ![0, 8, 11].contains(&compact(input.value).len()) => {
                Some(Problem::warning("A BIC has 8 or 11 letters and digits."))
            }
            _ => None,
        }
    }

    /// The account's label in keefast: its country and last four characters, as "DE •••• 3000".
    fn derive(facet: &str, fields: &Fields) -> Derived {
        let label = fields
            .text(IBAN)
            .map(compact)
            .filter(|iban| facet == FACET && iban.len() >= 8)
            .map(|iban| format!("{} •••• {}", &iban[..2], &iban[iban.len() - 4..]));
        Derived::new().label(label)
    }
}

keefast_plugin_sdk::export!(BankAccount);

/// Capitals, letters and digits only, as an IBAN or BIC is compared.
fn compact(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// What is wrong with an IBAN (ISO 13616): its shape, then its check digits (mod 97).
fn iban_problem(iban: &str) -> Option<Problem> {
    let shape = iban.len() >= 15
        && iban.len() <= 34
        && iban[..2].bytes().all(|b| b.is_ascii_uppercase())
        && iban[2..4].bytes().all(|b| b.is_ascii_digit());
    if !shape {
        return Some(Problem::error(
            "An IBAN starts with a country code and two digits, and has 15 to 34 characters.",
        ));
    }
    let rearranged = iban[4..].chars().chain(iban[..4].chars());
    let remainder = rearranged.fold(0u32, |remainder, c| {
        let value = c.to_digit(36).expect("compact keeps letters and digits");
        let shift = if value < 10 { 10 } else { 100 };
        (remainder * shift + value) % 97
    });
    (remainder != 1).then(|| Problem::error("This IBAN has a typo: its check digits don't match."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_iban_is_written_in_groups_of_four() {
        let input = Input::new(FACET, IBAN, "de89370400440532013000");
        assert_eq!(BankAccount::format(&input), "DE89 3704 0044 0532 0130 00");
    }

    #[test]
    fn an_iban_s_check_digits_are_checked() {
        let valid = Input::new(FACET, IBAN, "DE89 3704 0044 0532 0130 00");
        assert_eq!(BankAccount::validate(&valid), None);
        let typo = Input::new(FACET, IBAN, "DE89 3704 0044 0532 0130 01");
        assert!(BankAccount::validate(&typo).is_some());
        let short = Input::new(FACET, IBAN, "DE89");
        assert!(BankAccount::validate(&short).is_some());
    }

    #[test]
    fn the_label_shows_the_country_and_last_four() {
        let fields = Fields::of([(IBAN, Some("DE89 3704 0044 0532 0130 00")), (BIC, None)]);
        let derived = BankAccount::derive(FACET, &fields);
        assert_eq!(derived.label.as_deref(), Some("DE •••• 3000"));
    }
}
