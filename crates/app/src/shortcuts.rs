//! keefast's keyboard shortcuts as the user has them: the defaults (the UI's `shortcuts.json`)
//! with the keys the user gave some of them in Settings, kept in the app config. A new key is
//! refused when another of keefast's shortcuts, a running Plugin's command or every text field
//! has it already.

use std::collections::BTreeMap;

use keefast_plugins::Combo;
use keefast_plugins::shortcut::{TEXT_KEYS, keefast_by_name};

use crate::error::ApiError;

/// Shortcuts the user can't change: Escape and the arrows move through every list, menu and
/// dialog as well, whatever keefast's shortcuts say.
pub const FIXED: &[&str] = &["close", "nextEntry", "previousEntry"];

/// Keys no shortcut may have, which keep their usual jobs: closing, moving the focus, typing.
const KEPT_KEYS: &[&str] = &["escape", "tab", "space"];

/// Why a shortcut can't have the keys the user pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutRefusal {
    Unknown(String),
    Fixed,
    Invalid(String),
    /// Text fields have it: copy, paste, undo…
    TextKey(String),
    /// Another of keefast's shortcuts has it.
    Keefast(String),
    /// A running Plugin's command has it.
    Plugin {
        shortcut: String,
        plugin: String,
    },
}

impl From<ShortcutRefusal> for ApiError {
    fn from(refusal: ShortcutRefusal) -> Self {
        Self::invalid(match refusal {
            ShortcutRefusal::Unknown(name) => format!("keefast has no shortcut called {name}."),
            ShortcutRefusal::Fixed => "This shortcut can't be changed.".to_owned(),
            ShortcutRefusal::Invalid(keys) => {
                format!("{keys} can't be a shortcut: it keeps its usual job.")
            }
            ShortcutRefusal::TextKey(keys) => {
                format!("{keys} is taken by text fields (copy, paste, undo…).")
            }
            ShortcutRefusal::Keefast(keys) => {
                format!("{keys} is taken by another of keefast's shortcuts.")
            }
            ShortcutRefusal::Plugin { shortcut, plugin } => {
                format!("{shortcut} is taken by the {plugin} Plugin.")
            }
        })
    }
}

/// Every shortcut's keys: the user's where they changed them, the defaults elsewhere.
pub fn effective(changed: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    let mut shortcuts = keefast_by_name();
    for (name, keys) in &mut shortcuts {
        if let Some(mine) = changed.get(name) {
            keys.clone_from(mine);
        }
    }
    shortcuts
}

/// Checks that shortcut `name` can have `keys` ("Ctrl J"): that it is one the user can change,
/// and that nothing else has them. `plugin_holder` names the running Plugin that has a combo.
/// The keys as they are to be kept.
pub fn check(
    name: &str,
    keys: &str,
    changed: &BTreeMap<String, Vec<String>>,
    plugin_holder: impl Fn(&Combo) -> Option<String>,
) -> Result<String, ShortcutRefusal> {
    let shortcuts = effective(changed);
    let defaults = keefast_by_name();
    if !shortcuts.contains_key(name) {
        return Err(ShortcutRefusal::Unknown(name.to_owned()));
    }
    if FIXED.contains(&name) {
        return Err(ShortcutRefusal::Fixed);
    }
    let keys = keys.trim();
    let combo = Combo::parse(keys)
        .filter(|combo| !KEPT_KEYS.contains(&combo.key.as_str()))
        .ok_or_else(|| ShortcutRefusal::Invalid(keys.to_owned()))?;
    let same = |text: &String| Combo::parse(text).as_ref() == Some(&combo);
    // Ctrl C copies the password, as text fields let it when nothing is selected in them.
    let own_default = defaults[name].iter().any(same);
    if !own_default && TEXT_KEYS.iter().any(|text| same(&(*text).to_owned())) {
        return Err(ShortcutRefusal::TextKey(combo.to_string()));
    }
    let other = shortcuts
        .iter()
        .any(|(other, keys)| other != name && keys.iter().any(same));
    if other {
        return Err(ShortcutRefusal::Keefast(combo.to_string()));
    }
    if let Some(plugin) = plugin_holder(&combo) {
        return Err(ShortcutRefusal::Plugin {
            shortcut: combo.to_string(),
            plugin,
        });
    }
    Ok(keys.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &Combo) -> Option<String> {
        None
    }

    fn changed(name: &str, keys: &str) -> BTreeMap<String, Vec<String>> {
        BTreeMap::from([(name.to_owned(), vec![keys.to_owned()])])
    }

    #[test]
    fn the_users_keys_replace_a_shortcuts_defaults() {
        let shortcuts = effective(&changed("search", "Ctrl J"));
        assert_eq!(shortcuts["search"], ["Ctrl J"]);
        assert_eq!(shortcuts["palette"], ["Ctrl K", "Ctrl Shift P"]);
    }

    #[test]
    fn a_free_key_is_given() {
        let free = check("lockAll", " Ctrl Shift L ", &BTreeMap::new(), none);
        assert_eq!(free, Ok("Ctrl Shift L".to_owned()));
        // Its own default back, too.
        assert_eq!(
            check("search", "/", &changed("search", "Ctrl J"), none),
            Ok("/".to_owned())
        );
    }

    #[test]
    fn a_key_another_shortcut_has_is_refused() {
        assert_eq!(
            check("lockAll", "ctrl+k", &BTreeMap::new(), none),
            Err(ShortcutRefusal::Keefast("Ctrl K".to_owned()))
        );
        // Mod is Ctrl.
        assert_eq!(
            check("lockAll", "Mod S", &BTreeMap::new(), none),
            Err(ShortcutRefusal::Keefast("Ctrl S".to_owned()))
        );
        // A key the user moved a shortcut to is taken; the one it left is free.
        let moved = changed("search", "Ctrl J");
        assert_eq!(
            check("lockAll", "Ctrl J", &moved, none),
            Err(ShortcutRefusal::Keefast("Ctrl J".to_owned()))
        );
        assert_eq!(
            check("lockAll", "Ctrl F", &moved, none),
            Ok("Ctrl F".to_owned())
        );
    }

    #[test]
    fn a_key_a_plugin_has_is_refused() {
        let card = |combo: &Combo| (combo.to_string() == "Ctrl Shift C").then(|| "Card".to_owned());
        assert_eq!(
            check("lockAll", "Ctrl Shift C", &BTreeMap::new(), card),
            Err(ShortcutRefusal::Plugin {
                shortcut: "Ctrl Shift C".to_owned(),
                plugin: "Card".to_owned()
            })
        );
    }

    #[test]
    fn text_fields_keys_and_kept_keys_are_refused() {
        assert_eq!(
            check("lockAll", "Ctrl V", &BTreeMap::new(), none),
            Err(ShortcutRefusal::TextKey("Ctrl V".to_owned()))
        );
        // Copying the password keeps Ctrl C, its default.
        assert_eq!(
            check(
                "copyPassword",
                "Ctrl C",
                &changed("copyPassword", "Ctrl Shift W"),
                none
            ),
            Ok("Ctrl C".to_owned())
        );
        assert_eq!(
            check("lockAll", "Escape", &BTreeMap::new(), none),
            Err(ShortcutRefusal::Invalid("Escape".to_owned()))
        );
        assert_eq!(
            check("lockAll", "Ctrl", &BTreeMap::new(), none),
            Err(ShortcutRefusal::Invalid("Ctrl".to_owned()))
        );
    }

    #[test]
    fn unknown_and_fixed_shortcuts_cant_be_changed() {
        assert_eq!(
            check("nope", "Ctrl J", &BTreeMap::new(), none),
            Err(ShortcutRefusal::Unknown("nope".to_owned()))
        );
        assert_eq!(
            check("close", "Ctrl J", &BTreeMap::new(), none),
            Err(ShortcutRefusal::Fixed)
        );
    }
}
