//! keefast's Plugin host (ADR 0009). A Plugin is a manifest and a WASM component, run in
//! wasmtime behind the WIT interfaces in `wit/`. It never sees the Workspace: the host finds the
//! facets an Entry has from its custom fields' names, hands a hook the values of one facet's
//! Owned fields, and gets back text to show or values to write.
//!
//! keefast's own Card Plugin is compiled in as a signed package ([`PluginHost::bundled`],
//! `bundled.rs`). Others are installed from signed package files ([`package`]) and kept on disk by
//! the [`PluginStore`], which loads the enabled ones into the host. Both are checked the same way
//! at every load.
//!
//! What a Plugin adds to the Entry list and the command palette is declared too: a facet's label
//! as the list's subtitle ([`PluginHost::list_label`]), and commands with shortcuts the host
//! checks against keefast's own and other Plugins' whenever a Plugin loads or unloads
//! ([`shortcut`]), bundled or installed alike.
//!
//! A facet may also be filled into web forms by the Companion extension: the host lists those of
//! the running Plugins granted it ([`PluginHost::browser_fills`]), as custom field names for the
//! browser protocol to read, without running the Plugin.
//!
//! Plugin authors don't use this crate: they write against the SDK, `keefast-plugin-sdk`, and
//! package with the `keefast-plugin` command (docs/plugins/README.md).

mod bundled;
pub mod manifest;
pub mod package;
mod runtime;
pub mod shortcut;
mod store;

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use wasmtime::Engine;

pub use bundled::{bundled_by_release_key, bundled_key, is_release_key};
pub use manifest::{
    API_VERSION, Autocomplete, Capability, Command, Editor, Facet, FieldKind, Fill, FormKind,
    Manifest, OwnedField,
};
pub use package::{Package, PackageError, PublisherKey};
pub use runtime::{FUEL_PER_CALL, MEMORY_CAP, MonthYear, Problem, Severity};
pub use shortcut::{Combo, RefusedShortcut, ShortcutRefusal};
pub use store::{InstallError, InstalledPlugin, Offer, PluginStore, Publisher};

use bundled::bundled_plugins;
use runtime::{Derived, Field, Runtime};

/// The longest label or field text a Plugin can have shown; longer ones are cut.
pub const MAX_LABEL: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginError {
    #[error("the Plugin's manifest is invalid: {0}")]
    Manifest(String),
    #[error("the Plugin needs API version {0}; this keefast runs version {API_VERSION}")]
    ApiVersion(u32),
    #[error("a Plugin with the id {0} is already installed")]
    Taken(String),
    #[error("the Plugin owns the field \"{field}\", which {other} owns")]
    Clash { field: String, other: String },
    #[error("the Plugin's component can't run: {0}")]
    Component(String),
    #[error("the Plugin failed: {0}")]
    Hook(String),
    #[error("no Plugin {0} has that facet")]
    NoFacet(String),
}

/// A custom field of an Entry, as the host reads it to find facets.
#[derive(Debug, Clone, Copy)]
pub struct EntryField<'a> {
    pub name: &'a str,
    pub value: &'a str,
    pub protected: bool,
}

/// A facet an Entry has.
#[derive(Debug, Clone)]
pub struct FoundFacet<'h> {
    pub plugin: &'h Manifest,
    pub facet: &'h Facet,
    /// The facet's label from `derive`, if it gave one.
    pub label: Option<String>,
    /// Its Owned fields, in the manifest's order, whether the Entry has them or not.
    pub fields: Vec<FoundField<'h>>,
    /// Its command palette commands, in the manifest's order.
    pub commands: Vec<FoundCommand<'h>>,
}

/// A facet's command, with the shortcut the host let it have.
#[derive(Debug, Clone)]
pub struct FoundCommand<'h> {
    pub command: &'h Command,
    /// The Owned field it copies.
    pub owned: &'h OwnedField,
    /// As the UI writes it ("Ctrl Shift C"); `None` when it declared none or was refused one.
    pub shortcut: Option<&'h str>,
}

#[derive(Debug, Clone)]
pub struct FoundField<'h> {
    pub owned: &'h OwnedField,
    /// Its index in the Entry's custom fields; `None` when the Entry hasn't got it.
    pub index: Option<usize>,
    /// Text `derive` gave to show in place of its value.
    pub text: Option<String>,
}

/// A facet the Companion extension may fill web forms from, as the browser protocol needs it:
/// custom field names only. It never runs the Plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserFill {
    /// The Plugin's id.
    pub plugin: String,
    pub form: FormKind,
    /// An Entry has the facet when it has a custom field of each of these names.
    pub detect: Vec<String>,
    /// Each filled Owned field's custom field name, and the form field it goes into, in the
    /// manifest's order.
    pub fields: Vec<(String, Autocomplete)>,
}

struct Plugin {
    manifest: Manifest,
    runtime: Mutex<Runtime>,
    /// The shortcuts its commands got, by facet and command id, as the UI writes them.
    shortcuts: HashMap<(String, String), String>,
}

impl Plugin {
    fn runtime(&self) -> std::sync::MutexGuard<'_, Runtime> {
        // A panic mid-call leaves at worst an instance the next failed call replaces.
        self.runtime.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `derive` on one Entry's facet, `indexes` being where its Owned fields are in `fields`.
    /// Protected values reach it only with the `readProtected` Plugin capability. `None` when
    /// the hook failed.
    fn derive(
        &self,
        facet: &Facet,
        fields: &[EntryField],
        indexes: &[Option<usize>],
    ) -> Option<Derived> {
        let inputs: Vec<Field> = facet
            .fields
            .iter()
            .zip(indexes)
            .map(|(owned, index)| Field {
                key: owned.key.clone(),
                value: index.map(|index| fields[index]).and_then(|field| {
                    (!field.protected || self.manifest.has(Capability::ReadProtected))
                        .then(|| field.value.to_owned())
                }),
            })
            .collect();
        self.runtime().derive(&facet.id, &inputs).ok()
    }
}

/// The Plugins keefast runs.
pub struct PluginHost {
    engine: Engine,
    plugins: Vec<Plugin>,
    /// keefast's own shortcuts and the keys text fields own: no Plugin may have one.
    reserved: Vec<Combo>,
    /// The shortcuts Plugins declared and didn't get, in the order they were loaded.
    refused: Vec<RefusedShortcut>,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHost {
    /// A host with no Plugins, which keeps keefast's shortcuts and the keys text fields own from
    /// every Plugin loaded into it.
    pub fn new() -> Self {
        let keefast = shortcut::keefast();
        Self {
            engine: runtime::engine(),
            plugins: Vec::new(),
            reserved: shortcut::TEXT_KEYS
                .iter()
                .copied()
                .chain(keefast.iter().map(String::as_str))
                .filter_map(Combo::parse)
                .collect(),
            refused: Vec::new(),
        }
    }

    /// A host with the Plugins bundled with keefast whose packages pass their checks.
    pub fn bundled() -> Self {
        Self::bundled_reserving(&[])
    }

    /// [`Self::bundled`], keeping these shortcuts from the Plugins too.
    pub fn bundled_reserving(shortcuts: &[&str]) -> Self {
        let mut host = Self::new();
        host.reserve_shortcuts(shortcuts);
        for bundled in bundled_plugins() {
            if let Ok(package) = bundled.package {
                // Two bundled Plugins never clash: the bundled tests load them all.
                let _ = host.add(package.manifest, &package.component);
            }
        }
        host
    }

    /// Keeps these shortcuts from Plugins loaded from now on, as well as keefast's own.
    pub fn reserve_shortcuts(&mut self, shortcuts: &[&str]) {
        self.reserved
            .extend(shortcuts.iter().filter_map(|text| Combo::parse(text)));
    }

    /// The shortcuts Plugins declared and didn't get, and why: their commands are in the command
    /// palette without keys.
    pub fn refused_shortcuts(&self) -> &[RefusedShortcut] {
        &self.refused
    }

    /// Loads a Plugin, refusing one whose id is taken or whose Owned fields another owns. A
    /// shortcut one of its commands declares that keefast or a Plugin loaded earlier has is
    /// refused, not the Plugin: the command loads without keys, and [`Self::refused_shortcuts`]
    /// says why.
    pub fn load(&mut self, manifest: &str, component: &[u8]) -> Result<&Manifest, PluginError> {
        self.add(Manifest::parse(manifest)?, component)
    }

    /// Checks a component would load: that it compiles, is a Plugin of this API version and
    /// imports nothing.
    pub(crate) fn check_component(&self, component: &[u8]) -> Result<(), PluginError> {
        Runtime::new(&self.engine, component).map(|_| ())
    }

    /// Stops running a Plugin; its facets are no longer found. Whether it ran. The shortcuts it
    /// had are free again: the Plugins still running get theirs anew, in the order they loaded,
    /// so one refused a key the unloaded Plugin held gets it now.
    pub fn unload(&mut self, id: &str) -> bool {
        let before = self.plugins.len();
        self.plugins.retain(|plugin| plugin.manifest.id != id);
        let unloaded = self.plugins.len() != before;
        if unloaded {
            self.refused.clear();
            let plugins = std::mem::take(&mut self.plugins);
            for mut plugin in plugins {
                plugin.shortcuts = self.give_shortcuts(&plugin.manifest);
                self.plugins.push(plugin);
            }
        }
        unloaded
    }

    /// [`Self::load`] for a manifest already read.
    pub(crate) fn add(
        &mut self,
        manifest: Manifest,
        component: &[u8],
    ) -> Result<&Manifest, PluginError> {
        if self.manifest(&manifest.id).is_some() {
            return Err(PluginError::Taken(manifest.id));
        }
        for name in manifest.owned_names() {
            if let Some(other) = self.owner_of(name) {
                return Err(PluginError::Clash {
                    field: name.to_owned(),
                    other: other.name.clone(),
                });
            }
        }
        let runtime = Runtime::new(&self.engine, component)?;
        let shortcuts = self.give_shortcuts(&manifest);
        self.plugins.push(Plugin {
            manifest,
            runtime: Mutex::new(runtime),
            shortcuts,
        });
        Ok(&self.plugins.last().expect("pushed above").manifest)
    }

    /// The shortcuts a Plugin about to load may have, the others noted as refused.
    fn give_shortcuts(&mut self, manifest: &Manifest) -> HashMap<(String, String), String> {
        let mut given: HashMap<(String, String), String> = HashMap::new();
        let mut taken: Vec<Combo> = Vec::new();
        for facet in &manifest.contributions.facets {
            for command in &facet.commands {
                let Some(text) = &command.shortcut else {
                    continue;
                };
                let checked = Combo::parse_plugin(text).and_then(|combo| {
                    if self.reserved.contains(&combo) {
                        return Err(ShortcutRefusal::Keefast(combo.to_string()));
                    }
                    let owner = self.plugins.iter().find(|plugin| {
                        plugin
                            .shortcuts
                            .values()
                            .any(|other| Combo::parse(other).as_ref() == Some(&combo))
                    });
                    if let Some(owner) = owner {
                        return Err(ShortcutRefusal::Taken {
                            shortcut: combo.to_string(),
                            other: owner.manifest.name.clone(),
                        });
                    }
                    if taken.contains(&combo) {
                        return Err(ShortcutRefusal::Taken {
                            shortcut: combo.to_string(),
                            other: manifest.name.clone(),
                        });
                    }
                    Ok(combo)
                });
                match checked {
                    Ok(combo) => {
                        given.insert((facet.id.clone(), command.id.clone()), combo.to_string());
                        taken.push(combo);
                    }
                    Err(reason) => self.refused.push(RefusedShortcut {
                        plugin: manifest.id.clone(),
                        facet: facet.id.clone(),
                        command: command.id.clone(),
                        reason,
                    }),
                }
            }
        }
        given
    }

    pub fn manifests(&self) -> impl Iterator<Item = &Manifest> {
        self.plugins.iter().map(|plugin| &plugin.manifest)
    }

    pub fn manifest(&self, id: &str) -> Option<&Manifest> {
        self.plugin(id).map(|plugin| &plugin.manifest)
    }

    /// The Plugin owning a custom field of this name, if one does.
    pub fn owner_of(&self, name: &str) -> Option<&Manifest> {
        self.manifests()
            .find(|manifest| manifest.owned_names().any(|owned| owned == name))
    }

    fn plugin(&self, id: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|plugin| plugin.manifest.id == id)
    }

    /// The facets an Entry with these custom fields has, labelled by their Plugins. A Plugin
    /// whose `derive` fails still has its facets found, without labels.
    pub fn facets(&self, fields: &[EntryField]) -> Vec<FoundFacet<'_>> {
        let index_of = |name: &str| fields.iter().position(|field| field.name == name);
        let mut found = Vec::new();
        for plugin in &self.plugins {
            let manifest = &plugin.manifest;
            for facet in &manifest.contributions.facets {
                if !facet
                    .detect
                    .fields
                    .iter()
                    .all(|name| index_of(name).is_some())
                {
                    continue;
                }
                let indexes: Vec<_> = facet.fields.iter().map(|f| index_of(&f.name)).collect();
                let derived = plugin.derive(facet, fields, &indexes);
                let text_of = |key: &str| {
                    derived.as_ref().and_then(|derived| {
                        derived
                            .fields
                            .iter()
                            .find(|text| text.key == key)
                            .map(|text| shorten(&text.text))
                    })
                };
                found.push(FoundFacet {
                    plugin: manifest,
                    facet,
                    label: derived
                        .as_ref()
                        .and_then(|d| d.label.as_deref())
                        .map(shorten),
                    fields: facet
                        .fields
                        .iter()
                        .zip(indexes)
                        .map(|(owned, index)| FoundField {
                            owned,
                            index,
                            text: text_of(&owned.key),
                        })
                        .collect(),
                    commands: facet
                        .commands
                        .iter()
                        .filter_map(|command| {
                            Some(FoundCommand {
                                command,
                                owned: facet.fields.iter().find(|f| f.key == command.copy)?,
                                shortcut: plugin
                                    .shortcuts
                                    .get(&(facet.id.clone(), command.id.clone()))
                                    .map(String::as_str),
                            })
                        })
                        .collect(),
                });
            }
        }
        found
    }

    /// The facets of the running Plugins that the Companion extension may fill forms from: only
    /// Plugins granted the `browserFill` Plugin capability have any.
    pub fn browser_fills(&self) -> Vec<BrowserFill> {
        self.manifests()
            .filter(|manifest| manifest.has(Capability::BrowserFill))
            .flat_map(|manifest| {
                manifest.contributions.facets.iter().filter_map(|facet| {
                    let fill = facet.fill.as_ref()?;
                    Some(BrowserFill {
                        plugin: manifest.id.clone(),
                        form: fill.form,
                        detect: facet.detect.fields.clone(),
                        fields: facet
                            .fields
                            .iter()
                            .filter_map(|owned| {
                                Some((owned.name.clone(), *fill.fields.get(&owned.key)?))
                            })
                            .collect(),
                    })
                })
            })
            .collect()
    }

    /// Whether a custom field of this name is one the Entry list's subtitles are derived from,
    /// so the list collects only those before calling [`Self::list_label`].
    pub fn lists_by(&self, name: &str) -> bool {
        self.manifests().any(|manifest| {
            manifest
                .contributions
                .facets
                .iter()
                .filter(|facet| facet.subtitle)
                .any(|facet| facet.fields.iter().any(|field| field.name == name))
        })
    }

    /// The subtitle the Entry list shows for an Entry with these custom fields and no username:
    /// the label of its first facet, in the order the Plugins were loaded, that declares
    /// `subtitle` and has a label. Only those facets' `derive` runs, and it never gets the
    /// Workspace: the list collects the fields first ([`Self::lists_by`]), and calls this after
    /// letting the Workspace go.
    pub fn list_label(&self, fields: &[EntryField]) -> Option<String> {
        let index_of = |name: &str| fields.iter().position(|field| field.name == name);
        self.plugins.iter().find_map(|plugin| {
            plugin
                .manifest
                .contributions
                .facets
                .iter()
                .filter(|facet| facet.subtitle)
                .filter(|facet| {
                    facet
                        .detect
                        .fields
                        .iter()
                        .all(|name| index_of(name).is_some())
                })
                .find_map(|facet| {
                    let indexes: Vec<_> = facet.fields.iter().map(|f| index_of(&f.name)).collect();
                    plugin
                        .derive(facet, fields, &indexes)?
                        .label
                        .map(|label| shorten(&label))
                        .filter(|label| !label.is_empty())
                })
        })
    }

    /// A Plugin's facet, checked to exist.
    fn facet_of(&self, plugin: &str, facet: &str) -> Result<&Plugin, PluginError> {
        self.plugin(plugin)
            .filter(|found| found.manifest.facet(facet).is_some())
            .ok_or_else(|| PluginError::NoFacet(plugin.to_owned()))
    }

    /// A value as the user typed it into an Owned field, formatted by its Plugin.
    pub fn format(
        &self,
        plugin: &str,
        facet: &str,
        key: &str,
        value: &str,
    ) -> Result<String, PluginError> {
        self.facet_of(plugin, facet)?
            .runtime()
            .format(facet, key, value)
    }

    /// What the Plugin says is wrong with a value for an Owned field. `today` is the month it is
    /// now: Plugins have no clock.
    pub fn validate(
        &self,
        plugin: &str,
        facet: &str,
        key: &str,
        value: &str,
        today: MonthYear,
    ) -> Result<Option<Problem>, PluginError> {
        let mut problem = self
            .facet_of(plugin, facet)?
            .runtime()
            .validate(facet, key, value, today)?;
        if let Some(problem) = &mut problem {
            problem.message = shorten_to(&problem.message, 200);
        }
        Ok(problem)
    }
}

/// Text a Plugin has shown, cut to [`MAX_LABEL`] characters, without line breaks.
fn shorten(text: &str) -> String {
    shorten_to(text, MAX_LABEL)
}

fn shorten_to(text: &str, most: usize) -> String {
    let line: String = text
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(most + 1)
        .collect();
    if line.chars().count() > most {
        let mut cut: String = line.chars().take(most - 1).collect();
        cut.push('…');
        cut
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_cut() {
        assert_eq!(shorten("Visa •••• 1111"), "Visa •••• 1111");
        assert_eq!(shorten("one\ntwo"), "one");
        let long = "x".repeat(100);
        assert_eq!(shorten(&long).chars().count(), MAX_LABEL);
        assert!(shorten(&long).ends_with('…'));
    }
}
