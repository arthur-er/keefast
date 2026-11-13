//! Keyboard shortcuts Plugins declare for their command palette commands, and the ones they can't
//! have. A Plugin gets a shortcut only if neither keefast (its `SHORTCUTS`, and the keys text
//! fields own) nor a Plugin loaded before it has taken it; a refused shortcut leaves its command
//! in the command palette without keys.
//!
//! Combos are written as the UI's `lib/shortcuts.ts` writes them: "Ctrl Shift C".

use std::collections::BTreeMap;
use std::fmt;

/// keefast's own shortcuts: the UI's `SHORTCUTS`, from the file it reads them from, so every host
/// keeps them from Plugins, in the app as in `keefast-plugin pack` and `verify`.
pub fn keefast() -> Vec<String> {
    keefast_by_name().into_values().flatten().collect()
}

/// keefast's own shortcuts by name ("palette", "search"…), with their default keys, as
/// `shortcuts.json` gives them before the user changes any in Settings.
pub fn keefast_by_name() -> BTreeMap<String, Vec<String>> {
    serde_json::from_str(include_str!("../../app/ui/src/lib/shortcuts.json"))
        .expect("shortcuts.json lists keefast's shortcuts")
}

/// Keys every text field in the webview owns, which no Plugin may take even though keefast's
/// `SHORTCUTS` don't name them.
pub const TEXT_KEYS: &[&str] = &[
    "Ctrl A",
    "Ctrl C",
    "Ctrl V",
    "Ctrl X",
    "Ctrl Y",
    "Ctrl Z",
    "Ctrl Shift Z",
];

/// A key with its modifiers, compared as the UI's `matchesCombo` compares them. "Mod" counts as
/// Ctrl, so a combo keefast takes on one platform is taken on all.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    /// Lowercase: `c`, `,`, `arrowdown`.
    pub key: String,
}

impl Combo {
    /// Reads any combo the UI's `parseCombo` reads, as keefast's own are written. `None` for one
    /// without a key.
    pub fn parse(text: &str) -> Option<Self> {
        let mut combo = Combo {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            key: String::new(),
        };
        for part in text.split([' ', '+']).filter(|part| !part.is_empty()) {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" | "mod" => combo.ctrl = true,
                "shift" => combo.shift = true,
                "alt" | "option" => combo.alt = true,
                "cmd" | "meta" | "super" => combo.meta = true,
                key => combo.key = key.to_owned(),
            }
        }
        (!combo.key.is_empty()).then_some(combo)
    }

    /// Reads a combo a Plugin declares. Plugins may only have Ctrl with a letter, Ctrl Shift with
    /// a letter, or Ctrl with a digit: keys that never type text, and that the UI matches the
    /// same on every keyboard layout (Shift turns a digit into a symbol, and Alt a letter, on
    /// some).
    pub fn parse_plugin(text: &str) -> Result<Self, ShortcutRefusal> {
        let invalid = || ShortcutRefusal::Invalid(text.to_owned());
        let combo = Self::parse(text).ok_or_else(invalid)?;
        let mut key = combo.key.chars();
        let (Some(key), None) = (key.next(), key.next()) else {
            return Err(invalid());
        };
        let letter = key.is_ascii_lowercase();
        let digit = key.is_ascii_digit();
        if !combo.ctrl || combo.alt || combo.meta || !(letter || (digit && !combo.shift)) {
            return Err(invalid());
        }
        Ok(combo)
    }
}

impl fmt::Display for Combo {
    /// As the UI writes it: "Ctrl Shift C".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.meta {
            parts.push("Meta".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(if self.key.chars().count() == 1 {
            self.key.to_uppercase()
        } else {
            self.key.clone()
        });
        f.write_str(&parts.join(" "))
    }
}

/// Why a Plugin's command didn't get the shortcut it declared.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutRefusal {
    #[error(
        "\"{0}\" isn't a shortcut a Plugin can have: use Ctrl with a letter or a digit, or Ctrl Shift with a letter"
    )]
    Invalid(String),
    #[error("{0} is one of keefast's own shortcuts")]
    Keefast(String),
    #[error("{shortcut} is taken by the {other} Plugin")]
    Taken { shortcut: String, other: String },
}

/// A shortcut a Plugin declared and didn't get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedShortcut {
    pub plugin: String,
    pub facet: String,
    pub command: String,
    pub reason: ShortcutRefusal,
}

impl fmt::Display for RefusedShortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the {} Plugin's command {}/{} has no shortcut: {}",
            self.plugin, self.facet, self.command, self.reason
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keefasts_own_shortcuts_are_the_uis() {
        let shortcuts = keefast();
        for combo in [
            "Ctrl K",
            "Ctrl Shift P",
            "Ctrl B",
            "Ctrl Shift N",
            "Ctrl ,",
            "Escape",
        ] {
            assert!(shortcuts.iter().any(|s| s == combo), "{combo}");
        }
        for combo in &shortcuts {
            assert!(Combo::parse(combo).is_some(), "{combo}");
        }
    }

    #[test]
    fn keefasts_combos_read_as_the_ui_reads_them() {
        let combo = Combo::parse("Ctrl Shift P").unwrap();
        assert!(combo.ctrl && combo.shift && !combo.alt && combo.key == "p");
        assert_eq!(Combo::parse("Mod+k"), Combo::parse("Ctrl K"));
        assert_eq!(Combo::parse("ArrowDown").unwrap().key, "arrowdown");
        assert_eq!(Combo::parse("Ctrl ,").unwrap().to_string(), "Ctrl ,");
        assert_eq!(Combo::parse("Ctrl"), None);
    }

    #[test]
    fn plugins_have_ctrl_with_a_letter_or_digit() {
        for good in ["Ctrl Shift C", "ctrl+shift+c", "Ctrl 1", "Ctrl J"] {
            assert!(Combo::parse_plugin(good).is_ok(), "{good}");
        }
        assert_eq!(
            Combo::parse_plugin("ctrl+shift+c").unwrap().to_string(),
            "Ctrl Shift C"
        );
        for bad in [
            "C",
            "Shift C",
            "Alt C",
            "Ctrl Alt C",
            "Ctrl Meta C",
            "Ctrl Shift 1",
            "Ctrl Enter",
            "Ctrl ,",
            "Ctrl",
            "",
        ] {
            assert!(
                matches!(Combo::parse_plugin(bad), Err(ShortcutRefusal::Invalid(_))),
                "{bad}"
            );
        }
    }
}
