//! A payment card's rules: its brand, how its number and expiry are written, and what is wrong
//! with them. Plain Rust, tested natively; `lib.rs` exposes it as the Plugin's hooks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brand {
    Visa,
    Mastercard,
    Amex,
    Elo,
    Discover,
    Unknown,
}

impl Brand {
    pub fn name(self) -> &'static str {
        match self {
            Self::Visa => "Visa",
            Self::Mastercard => "Mastercard",
            Self::Amex => "American Express",
            Self::Elo => "Elo",
            Self::Discover => "Discover",
            Self::Unknown => "Card",
        }
    }
}

/// The digits of `text`, ignoring spaces, dashes and anything else.
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// The brand a number's first digits say it is. Elo's ranges come first: some start with 4 or 5.
pub fn brand_of(number: &str) -> Brand {
    const ELO: [&str; 15] = [
        "4011", "4312", "4389", "4514", "4576", "5041", "5066", "5067", "509", "6277", "6362",
        "6363", "650", "6516", "6550",
    ];
    let n = digits(number);
    let starts = |prefix: &str| n.starts_with(prefix);
    if ELO.iter().any(|prefix| starts(prefix)) {
        Brand::Elo
    } else if starts("34") || starts("37") {
        Brand::Amex
    } else if starts("4") {
        Brand::Visa
    } else if [
        "51", "52", "53", "54", "55", "22", "23", "24", "25", "26", "27",
    ]
    .iter()
    .any(|prefix| starts(prefix))
    {
        Brand::Mastercard
    } else if starts("6011") || starts("65") {
        Brand::Discover
    } else {
        Brand::Unknown
    }
}

/// A number in groups as printed on the card: 4-4-4-4, or American Express's 4-6-5.
pub fn format_number(text: &str) -> String {
    let n: String = digits(text).chars().take(19).collect();
    let groups: &[usize] = if brand_of(&n) == Brand::Amex {
        &[4, 6, 5]
    } else {
        &[4, 4, 4, 4, 3]
    };
    let mut out = String::new();
    let mut rest = n.as_str();
    for &size in groups {
        if rest.is_empty() {
            break;
        }
        let (group, tail) = rest.split_at(size.min(rest.len()));
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(group);
        rest = tail;
    }
    out
}

/// Whether a number passes the Luhn check every card number does.
pub fn luhn(number: &str) -> bool {
    let n = digits(number);
    if n.len() < 12 {
        return false;
    }
    let sum: u32 = n
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, byte)| {
            let d = u32::from(byte - b'0');
            if i % 2 == 1 {
                if d * 2 > 9 { d * 2 - 9 } else { d * 2 }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// "Visa •••• 1111": the brand and last four digits, which say which card it is without saying
/// its number. `None` until there are four digits.
pub fn label(number: &str) -> Option<String> {
    let n = digits(number);
    let last = n.get(n.len().checked_sub(4)?..)?;
    Some(format!("{} •••• {last}", brand_of(&n).name()))
}

/// A typed expiry as `MM/YY`, the slash added as the user types.
pub fn format_expiry(text: &str) -> String {
    let n: String = digits(text).chars().take(4).collect();
    match n.len() {
        1 if n.as_bytes()[0] > b'1' => format!("0{n}/"),
        3 | 4 => format!("{}/{}", &n[..2], &n[2..]),
        _ => n,
    }
}

/// Whether an expiry is a month as `MM/YY`.
pub fn is_month_year(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 5
        && bytes[2] == b'/'
        && [0, 1, 3, 4].iter().all(|&i| bytes[i].is_ascii_digit())
        && matches!(text[..2].parse::<u8>(), Ok(1..=12))
}

/// The month an `MM/YY` expiry is, as `(year, month)`, if it is one.
pub fn month_year(text: &str) -> Option<(u16, u8)> {
    is_month_year(text).then(|| {
        let month = text[..2].parse().expect("checked to be digits");
        let year: u16 = text[3..].parse().expect("checked to be digits");
        (2000 + year, month)
    })
}

/// Months from `today` to `month`: 0 for this month, negative for one that has passed.
fn months_until(month: (u16, u8), today: (u16, u8)) -> i32 {
    let index = |(year, month): (u16, u8)| i32::from(year) * 12 + i32::from(month);
    index(month) - index(today)
}

/// What is wrong with a field's value, if anything: `(is an error, message)`. `today` is the
/// month it is now, `(year, month)`, which the host passes `validate`: a Plugin has no clock.
/// Without it, only an expiry's shape is checked. A card is good through the last day of its
/// expiry month, and expires soon in that month or the one before. Everything here is a
/// warning: the user may know better, and saving still works.
pub fn problem(key: &str, value: &str, today: Option<(u16, u8)>) -> Option<(bool, &'static str)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let only_digits =
        |allowed: fn(char) -> bool| value.chars().all(|c| c.is_ascii_digit() || allowed(c));
    let warning = |message| Some((false, message));
    match key {
        "number" if !only_digits(|c| c == ' ' || c == '-') => {
            warning("A card number has only digits.")
        }
        "number" if !(12..=19).contains(&digits(value).len()) || !luhn(value) => {
            warning("Doesn't look like a valid card number. Check the digits.")
        }
        "expiry" => match month_year(value).map(|month| today.map(|today| months_until(month, today))) {
            None => warning("Type the expiry as MM/YY, such as 08/31."),
            Some(None) => None,
            Some(Some(..0)) => warning("This card has expired. Type the new card's expiry."),
            Some(Some(0..=1)) => warning("This card expires soon."),
            Some(_) => None,
        },
        "cvv" if !only_digits(|_| false) || !(3..=4).contains(&value.len()) => {
            warning("A CVV is 3 or 4 digits.")
        }
        "pin" if !only_digits(|_| false) || !(4..=12).contains(&value.len()) => {
            warning("A PIN is 4 to 12 digits.")
        }
        _ => None,
    }
}

/// A value as the user typed it, formatted as the field keeps it.
pub fn format(key: &str, value: &str) -> String {
    match key {
        "number" => format_number(value),
        "expiry" => format_expiry(value),
        "cvv" => digits(value).chars().take(4).collect(),
        "pin" => digits(value).chars().take(12).collect(),
        _ => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brands_come_from_the_first_digits() {
        assert_eq!(brand_of("4111 1111 1111 1111"), Brand::Visa);
        assert_eq!(brand_of("5162 9200 1234 5676"), Brand::Mastercard);
        assert_eq!(brand_of("2221 0000 0000 0009"), Brand::Mastercard);
        assert_eq!(brand_of("3782 822463 10005"), Brand::Amex);
        assert_eq!(brand_of("6011 1111 1111 1117"), Brand::Discover);
        assert_eq!(brand_of("4011 7800 0000 0000"), Brand::Elo);
        assert_eq!(brand_of("9999"), Brand::Unknown);
    }

    #[test]
    fn numbers_are_grouped_as_printed() {
        assert_eq!(format_number("4111111111111111"), "4111 1111 1111 1111");
        assert_eq!(format_number("378282246310005"), "3782 822463 10005");
        assert_eq!(format_number("41111"), "4111 1");
        assert_eq!(format_number("4111-1111"), "4111 1111");
        assert_eq!(format_number(""), "");
    }

    #[test]
    fn the_label_is_the_brand_and_last_four_digits() {
        assert_eq!(
            label("4111 1111 1111 1111").as_deref(),
            Some("Visa •••• 1111")
        );
        assert_eq!(label("9999 0000 1234").as_deref(), Some("Card •••• 1234"));
        assert_eq!(label("411"), None);
    }

    #[test]
    fn luhn_catches_a_mistyped_digit() {
        assert!(luhn("4111 1111 1111 1111"));
        assert!(!luhn("4111 1111 1111 1112"));
        assert!(!luhn("4111"));
    }

    #[test]
    fn expiries_are_month_and_year() {
        assert_eq!(format_expiry("8"), "08/");
        assert_eq!(format_expiry("1"), "1");
        assert_eq!(format_expiry("0831"), "08/31");
        assert_eq!(format_expiry("08/3"), "08/3");
        assert!(is_month_year("08/31"));
        assert!(!is_month_year("13/31"));
        assert!(!is_month_year("8/31"));
    }

    const TODAY: Option<(u16, u8)> = Some((2026, 10));

    #[test]
    fn problems_say_what_to_fix() {
        let problem = |key, value| problem(key, value, TODAY);
        assert_eq!(problem("number", "4111 1111 1111 1111"), None);
        assert_eq!(
            problem("number", "4111 1111 1111 1112").map(|p| p.0),
            Some(false)
        );
        assert_eq!(problem("number", "4111 abcd").map(|p| p.0), Some(false));
        assert_eq!(problem("expiry", "08/31"), None);
        assert_eq!(problem("expiry", "8/31").map(|p| p.0), Some(false));
        assert_eq!(
            problem("cvv", "12"),
            Some((false, "A CVV is 3 or 4 digits."))
        );
        assert_eq!(problem("pin", "8271"), None);
        assert_eq!(problem("holder", "anything"), None);
        assert_eq!(problem("cvv", ""), None);
    }

    #[test]
    fn an_expiry_is_checked_against_today() {
        let message = |value| problem("expiry", value, TODAY).map(|p| p.1);
        assert_eq!(
            message("09/26"),
            Some("This card has expired. Type the new card's expiry.")
        );
        assert_eq!(message("12/25"), message("09/26"));
        assert_eq!(message("10/26"), Some("This card expires soon."));
        assert_eq!(message("11/26"), Some("This card expires soon."));
        assert_eq!(message("12/26"), None);
        assert_eq!(
            problem("expiry", "01/27", Some((2026, 12))).map(|p| p.1),
            Some("This card expires soon.")
        );
        assert_eq!(month_year("08/31"), Some((2031, 8)));
        assert_eq!(month_year("13/31"), None);
    }
}
