//! Settings → Plugins: the Plugins installed, installing one from a package file (ADR 0009), and
//! enabling, disabling and removing them. Installing takes two steps, so the user sees the Plugin
//! capabilities a package asks for before anything is installed: `choose_plugin_package` reads
//! and checks a package and says what installing it would do, and `install_plugin` installs that
//! very package once the user agrees.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use keefast_plugins::{
    Capability, InstallError, InstalledPlugin, Offer, PluginHost, Publisher, package::MAX_PACKAGE,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use super::{Core, blocking, file_dialog, local_path};
use crate::error::{ApiError, ApiResult, ErrorKind};

/// Something a Plugin may do, granted when it is installed. Its Owned fields are listed apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginCapability {
    /// Its hooks see the protected values of its Owned fields, to label them.
    ReadProtected,
    /// The user can copy its Owned fields.
    Copy,
}

impl From<Capability> for PluginCapability {
    fn from(capability: Capability) -> Self {
        match capability {
            Capability::ReadProtected => Self::ReadProtected,
            Capability::Copy => Self::Copy,
        }
    }
}

/// Who made a Plugin, as far as keefast can tell.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PluginPublisher {
    /// keefast itself, with its signing key's fingerprint. `release` is false only for the
    /// Plugins bundled with a development or source build, signed by a key it made for itself
    /// rather than keefast's release key.
    Keefast { fingerprint: String, release: bool },
    /// The name the Plugin gives, which keefast can't check, and its key's fingerprint
    /// (`3F2A 91C0 7B1E 44D2`).
    Unverified { name: String, fingerprint: String },
}

impl From<Publisher> for PluginPublisher {
    fn from(publisher: Publisher) -> Self {
        match publisher {
            Publisher::Keefast {
                fingerprint,
                release,
            } => Self::Keefast {
                fingerprint,
                release,
            },
            Publisher::Unverified { name, fingerprint } => Self::Unverified { name, fingerprint },
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub id: String,
    pub name: String,
    /// `None` when its package can't be read any more.
    pub version: Option<String>,
    pub publisher: PluginPublisher,
    /// Comes with keefast: it can be disabled, not removed.
    pub bundled: bool,
    pub enabled: bool,
    /// Enabled, and running.
    pub running: bool,
    /// The Plugin capabilities the user agreed to.
    pub capabilities: Vec<PluginCapability>,
    /// The custom fields it owns.
    pub owned_fields: Vec<String>,
    /// Its icon, as a `data:` URL.
    pub icon: Option<String>,
    /// Why it doesn't run though enabled.
    pub problem: Option<String>,
    /// The shortcuts its commands declared and didn't get, each a sentence saying why, such as
    /// "Copy card number has no shortcut: Ctrl Shift C is one of keefast's own shortcuts." The
    /// commands are still in the command palette, without keys.
    pub refused_shortcuts: Vec<String>,
}

impl From<InstalledPlugin> for PluginView {
    fn from(plugin: InstalledPlugin) -> Self {
        Self {
            id: plugin.id,
            name: plugin.name,
            version: plugin.version,
            publisher: plugin.publisher.into(),
            bundled: plugin.bundled,
            enabled: plugin.enabled,
            running: plugin.running,
            capabilities: plugin.capabilities.into_iter().map(Into::into).collect(),
            owned_fields: plugin.owned_fields,
            icon: plugin.icon.as_deref().map(data_url),
            problem: plugin.problem,
            refused_shortcuts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginList {
    /// keefast's own first, then the installed ones by name.
    pub plugins: Vec<PluginView>,
    /// Why the installed Plugins couldn't be read, if they couldn't.
    pub error: Option<String>,
}

/// What installing a package would do, for the user to agree to.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginOffer {
    /// The package file, for `install_plugin`.
    pub path: String,
    /// Names exactly this package, for `install_plugin`.
    pub digest: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: PluginPublisher,
    /// keefast hasn't seen this publisher key before, and will trust it from now on for this
    /// Plugin's updates.
    pub new_key: bool,
    /// The version installed now, if this is an update.
    pub installed_version: Option<String>,
    pub capabilities: Vec<PluginCapability>,
    pub owned_fields: Vec<String>,
    /// What it asks for that the user hasn't agreed to: everything, for a new Plugin.
    pub new_capabilities: Vec<PluginCapability>,
    pub new_owned_fields: Vec<String>,
    pub icon: Option<String>,
}

impl PluginOffer {
    fn new(path: String, offer: Offer) -> Self {
        Self {
            path,
            digest: offer.digest,
            id: offer.id,
            name: offer.name,
            version: offer.version,
            publisher: offer.publisher.into(),
            new_key: offer.new_key,
            installed_version: offer.installed_version,
            capabilities: offer.capabilities.into_iter().map(Into::into).collect(),
            owned_fields: offer.owned_fields,
            new_capabilities: offer.new_capabilities.into_iter().map(Into::into).collect(),
            new_owned_fields: offer.new_owned_fields,
            icon: offer.icon.as_deref().map(data_url),
        }
    }
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", BASE64.encode(png))
}

impl From<InstallError> for ApiError {
    fn from(error: InstallError) -> Self {
        match error {
            InstallError::Package(error) => {
                ApiError::invalid("This file isn't a Plugin package keefast can install.")
                    .with_detail(sentence(&error.to_string()))
            }
            InstallError::Plugin(error) => ApiError::invalid("This Plugin can't be installed.")
                .with_detail(sentence(&error.to_string())),
            InstallError::NotInstalled => ApiError::new(ErrorKind::NotFound, error.to_string()),
            InstallError::NoStore => ApiError::new(ErrorKind::Unavailable, error.to_string()),
            InstallError::Io(_) => ApiError::new(ErrorKind::Io, error.to_string()),
            InstallError::KeyChanged { .. }
            | InstallError::Reserved
            | InstallError::BundledId(_)
            | InstallError::Clash { .. }
            | InstallError::Changed
            | InstallError::Bundled(_) => ApiError::invalid(error.to_string()),
        }
    }
}

/// A host or package error as a sentence: capitalized, with a full stop.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut sentence: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if !sentence.ends_with('.') {
        sentence.push('.');
    }
    sentence
}

#[tauri::command]
#[specta::specta]
pub async fn list_plugins(core: Core<'_>) -> ApiResult<PluginList> {
    Ok(core.read_plugins(|store, host| PluginList {
        plugins: store
            .list(host)
            .into_iter()
            .map(|plugin| {
                let mut view = PluginView::from(plugin);
                view.refused_shortcuts = refused_shortcuts(host, &view.id);
                view
            })
            .collect(),
        error: store.error().map(str::to_owned),
    }))
}

/// Why a running Plugin's commands didn't get the shortcuts they declared, by command title.
fn refused_shortcuts(host: &PluginHost, plugin: &str) -> Vec<String> {
    host.refused_shortcuts()
        .iter()
        .filter(|refused| refused.plugin == plugin)
        .map(|refused| {
            let title = host
                .manifest(plugin)
                .and_then(|manifest| manifest.facet(&refused.facet))
                .and_then(|facet| facet.commands.iter().find(|c| c.id == refused.command))
                .map_or(refused.command.as_str(), |command| command.title.as_str());
            sentence(&format!("{title} has no shortcut: {}", refused.reason))
        })
        .collect()
}

/// Asks the user for a Plugin package and says what installing it would do; `None` if they
/// chose none. Installs nothing. A package keefast would refuse fails here, with why.
#[tauri::command]
#[specta::specta]
pub async fn choose_plugin_package(
    app: AppHandle,
    core: Core<'_>,
) -> ApiResult<Option<PluginOffer>> {
    let picked = blocking(move || {
        file_dialog(&app, "Install a Plugin")
            .add_filter("keefast Plugin", &["kfplugin"])
            .blocking_pick_file()
    })
    .await?;
    let Some(path) = local_path(picked)? else {
        return Ok(None);
    };
    let text = path
        .to_str()
        .ok_or_else(|| ApiError::invalid("keefast can't open a file with this name."))?
        .to_owned();
    let bytes = read(&path).await?;
    let offer = core.read_plugins(|store, host| store.inspect(host, &bytes))?;
    Ok(Some(PluginOffer::new(text, offer)))
}

/// Installs the package `choose_plugin_package` showed, or updates the installed Plugin to it,
/// if it is still the package with `digest`.
#[tauri::command]
#[specta::specta]
pub async fn install_plugin(core: Core<'_>, path: String, digest: String) -> ApiResult<()> {
    let bytes = read(Path::new(&path)).await?;
    core.change_plugins(|store, host| store.install(host, &bytes, &digest))?;
    Ok(())
}

/// Enables or disables a Plugin. A disabled Plugin's data shows as ordinary custom fields.
#[tauri::command]
#[specta::specta]
pub async fn set_plugin_enabled(core: Core<'_>, id: String, enabled: bool) -> ApiResult<()> {
    core.change_plugins(|store, host| store.set_enabled(host, &id, enabled))?;
    Ok(())
}

/// Removes an installed Plugin. The data it kept stays in the Vaults as ordinary custom fields.
#[tauri::command]
#[specta::specta]
pub async fn remove_plugin(core: Core<'_>, id: String) -> ApiResult<()> {
    core.change_plugins(|store, host| store.remove(host, &id))?;
    Ok(())
}

/// A package file, read on a blocking thread.
async fn read(path: &Path) -> ApiResult<Vec<u8>> {
    let path = path.to_owned();
    blocking(move || {
        let too_big = std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_PACKAGE);
        if too_big {
            return Err(ApiError::invalid(
                "This file is too large to be a Plugin package.",
            ));
        }
        std::fs::read(&path).map_err(|error| {
            ApiError::new(ErrorKind::Io, "The Plugin package could not be read.")
                .with_detail(error.to_string())
        })
    })
    .await?
}

#[cfg(test)]
mod tests {
    use keefast_core::CustomField;
    use keefast_plugins::{InstallError, PluginHost, PluginStore};
    use secrecy::SecretString;

    use super::sentence;
    use crate::commands::entries::facet_views;
    use crate::core::tests::core;
    use crate::error::{ApiError, ErrorKind};

    fn card() -> Vec<CustomField> {
        let field = |name: &str, value: &str, protected| CustomField {
            name: name.to_owned(),
            value: SecretString::from(value),
            protected,
        };
        vec![
            field("Card number", "4111 1111 1111 1111", true),
            field("Cardholder", "ARTHUR REZENDE", false),
        ]
    }

    #[test]
    fn a_disabled_plugin_leaves_its_data_to_the_custom_fields_and_the_ui_is_told() {
        let dir = tempfile::tempdir().unwrap();
        let (core, _) = core();
        core.use_plugin_store(PluginStore::open(dir.path()));
        assert_eq!(facet_views(&core.plugins(), &card()).len(), 1);

        let before = core.revision();
        core.change_plugins(|store, host| store.set_enabled(host, "keefast.card", false))
            .unwrap();
        assert!(facet_views(&core.plugins(), &card()).is_empty());
        assert!(core.revision() > before);
        let listed = core.read_plugins(|store, host| store.list(host));
        assert!(!listed[0].enabled && !listed[0].running);

        core.change_plugins(|store, host| store.set_enabled(host, "keefast.card", true))
            .unwrap();
        assert_eq!(facet_views(&core.plugins(), &card()).len(), 1);
    }

    #[test]
    fn settings_say_why_a_command_has_no_shortcut() {
        let host = PluginHost::bundled_reserving(&["Ctrl Shift C"]);
        assert_eq!(
            super::refused_shortcuts(&host, "keefast.card"),
            ["Copy card number has no shortcut: Ctrl Shift C is one of keefast's own shortcuts."]
        );
        assert!(super::refused_shortcuts(&host, "acme.iban").is_empty());
    }

    #[test]
    fn the_installed_plugins_are_kept_from_keefasts_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        let (core, _) = core();
        core.use_plugin_store(PluginStore::open(dir.path()));
        assert!(core.plugins().refused_shortcuts().is_empty());
        // The store's host reserves keefast's keys, for any Plugin loaded into it.
        let mut json: serde_json::Value =
            serde_json::from_str(include_str!("../../../../plugins/card/plugin.json")).unwrap();
        json["id"] = "acme.cards".into();
        json["contributions"]["facets"][0]["fields"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .for_each(|field| {
                let name = format!("Acme {}", field["name"].as_str().unwrap());
                field["name"] = name.into();
            });
        json["contributions"]["facets"][0]["detect"]["fields"] =
            serde_json::json!(["Acme Card number"]);
        json["contributions"]["facets"][0]["commands"][0]["shortcut"] = "Ctrl B".into();
        core.change_plugins(|_, host| {
            host.load(
                &json.to_string(),
                include_bytes!("../../../../plugins/card/card.wasm"),
            )
            .map(|_| ())
        })
        .unwrap();
        assert_eq!(
            core.plugins().refused_shortcuts()[0].reason.to_string(),
            "Ctrl B is one of keefast's own shortcuts"
        );
    }

    #[test]
    fn refusals_are_whole_sentences() {
        let clash = ApiError::from(InstallError::Clash {
            name: "Acme cards".into(),
            field: "Card number".into(),
            other: "Card".into(),
        });
        assert_eq!(clash.kind, ErrorKind::Invalid);
        assert!(
            clash
                .message
                .starts_with("\"Acme cards\" would keep its data")
        );
        let bad = ApiError::from(InstallError::Package(
            keefast_plugins::PackageError::BadSignature,
        ));
        assert_eq!(
            bad.message,
            "This file isn't a Plugin package keefast can install."
        );
        assert!(bad.detail.unwrap().starts_with("The package's signature"));
        assert_eq!(sentence("no dot"), "No dot.");
    }
}
