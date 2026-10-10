//! keefast's Plugin host (ADR 0009). A Plugin is a manifest and a WASM component, run in
//! wasmtime behind the WIT interfaces in `wit/`. It never sees the Workspace: the host finds the
//! facets an Entry has from its custom fields' names, hands a hook the values of one facet's
//! Owned fields, and gets back text to show or values to write.
//!
//! keefast's own Card Plugin is compiled in ([`PluginHost::bundled`]). Others are installed
//! from signed package files ([`package`]) and kept on disk by the [`PluginStore`], which loads
//! the enabled ones into the host.

pub mod manifest;
pub mod package;
mod runtime;
mod store;

use std::sync::{Mutex, PoisonError};

use wasmtime::Engine;

pub use manifest::{API_VERSION, Capability, Facet, FieldKind, Manifest, OwnedField};
pub use package::{Package, PackageError, PublisherKey};
pub use runtime::{FUEL_PER_CALL, MEMORY_CAP, Problem, Severity};
pub use store::{InstallError, InstalledPlugin, Offer, PluginStore, Publisher};

use runtime::{Field, Runtime};

/// The longest label or field text a Plugin can have shown; longer ones are cut.
pub const MAX_LABEL: usize = 64;

/// Plugins compiled into keefast: their manifest and component.
const BUNDLED: &[(&str, &[u8])] = &[(
    include_str!("../../../plugins/card/plugin.json"),
    include_bytes!("../../../plugins/card/card.wasm"),
)];

/// The Plugins compiled into keefast, read.
pub(crate) fn bundled_plugins() -> impl Iterator<Item = (Manifest, &'static [u8])> {
    BUNDLED.iter().map(|(manifest, component)| {
        let manifest = Manifest::parse(manifest).expect("a bundled Plugin's manifest reads");
        (manifest, *component)
    })
}

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
}

#[derive(Debug, Clone)]
pub struct FoundField<'h> {
    pub owned: &'h OwnedField,
    /// Its index in the Entry's custom fields; `None` when the Entry hasn't got it.
    pub index: Option<usize>,
    /// Text `derive` gave to show in place of its value.
    pub text: Option<String>,
}

struct Plugin {
    manifest: Manifest,
    runtime: Mutex<Runtime>,
}

impl Plugin {
    fn runtime(&self) -> std::sync::MutexGuard<'_, Runtime> {
        // A panic mid-call leaves at worst an instance the next failed call replaces.
        self.runtime.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The Plugins keefast runs.
pub struct PluginHost {
    engine: Engine,
    plugins: Vec<Plugin>,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHost {
    /// A host with no Plugins.
    pub fn new() -> Self {
        Self {
            engine: runtime::engine(),
            plugins: Vec::new(),
        }
    }

    /// A host with the Plugins compiled into keefast.
    pub fn bundled() -> Self {
        let mut host = Self::new();
        for (manifest, component) in bundled_plugins() {
            host.add(manifest, component)
                .expect("a bundled Plugin loads");
        }
        host
    }

    /// Loads a Plugin, refusing one whose id is taken or whose Owned fields another owns.
    pub fn load(&mut self, manifest: &str, component: &[u8]) -> Result<&Manifest, PluginError> {
        self.add(Manifest::parse(manifest)?, component)
    }

    /// Checks a component would load: that it compiles, is a Plugin of this API version and
    /// imports nothing.
    pub(crate) fn check_component(&self, component: &[u8]) -> Result<(), PluginError> {
        Runtime::new(&self.engine, component).map(|_| ())
    }

    /// Stops running a Plugin; its facets are no longer found. Whether it ran.
    pub fn unload(&mut self, id: &str) -> bool {
        let before = self.plugins.len();
        self.plugins.retain(|plugin| plugin.manifest.id != id);
        self.plugins.len() != before
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
        self.plugins.push(Plugin {
            manifest,
            runtime: Mutex::new(runtime),
        });
        Ok(&self.plugins.last().expect("pushed above").manifest)
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
                let inputs: Vec<Field> = facet
                    .fields
                    .iter()
                    .zip(&indexes)
                    .map(|(owned, index)| Field {
                        key: owned.key.clone(),
                        value: index.map(|index| fields[index]).and_then(|field| {
                            (!field.protected || manifest.has(Capability::ReadProtected))
                                .then(|| field.value.to_owned())
                        }),
                    })
                    .collect();
                let derived = plugin.runtime().derive(&facet.id, &inputs).ok();
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
                });
            }
        }
        found
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

    /// What the Plugin says is wrong with a value for an Owned field.
    pub fn validate(
        &self,
        plugin: &str,
        facet: &str,
        key: &str,
        value: &str,
    ) -> Result<Option<Problem>, PluginError> {
        let mut problem = self
            .facet_of(plugin, facet)?
            .runtime()
            .validate(facet, key, value)?;
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
