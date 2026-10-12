//! Plugins' facets in Entry editor A (ADR 0009): which facets the editor can add to an Entry, and
//! the Plugins' hooks that format and check what the user types into their Owned fields.
//!
//! The editor reads an Entry's facets from its `EntryDetail`, and saves their Owned fields as the
//! custom fields they are, in `EntryInput`: nothing here touches the Workspace.

use std::time::SystemTime;

use keefast_core::Timestamp;
use keefast_plugins::{Editor, Facet, Manifest, MonthYear, PluginError, Severity};
use serde::Serialize;
use specta::Type;

use super::Core;
use super::entries::OwnedFieldKind;
use crate::error::{ApiError, ApiResult, ErrorKind};

/// One of a facet's Owned fields, as its Plugin declares it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnedFieldTemplate {
    /// How its Plugin names it.
    pub key: String,
    /// The custom field's name, such as `Card number`.
    pub name: String,
    /// What the section calls it, such as "Number".
    pub label: String,
    pub kind: OwnedFieldKind,
    /// Whether a new value is kept protected.
    pub protected: bool,
    /// What its input shows while empty, such as "MM/YY".
    pub placeholder: Option<String>,
}

/// A facet's section in the Entry editor: the actions adding it and taking it off an Entry.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FacetEditorView {
    /// Such as "Add card".
    pub add: String,
    /// Such as "Remove card".
    pub remove: String,
}

impl From<&Editor> for FacetEditorView {
    fn from(editor: &Editor) -> Self {
        Self {
            add: editor.add.clone(),
            remove: editor.remove.clone(),
        }
    }
}

/// A facet a Plugin contributes, whether an Entry has it or not, such as the Card.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FacetTemplate {
    /// The Plugin's id, such as `keefast.card`.
    pub plugin: String,
    /// The facet's id within the Plugin, such as `card`.
    pub facet: String,
    /// The section's title, such as "Card".
    pub title: String,
    /// One of the kit's icons, by name (`credit-card`).
    pub icon: Option<String>,
    /// The names of the fields an Entry has the facet by.
    pub detect: Vec<String>,
    /// Its section in the Entry editor; `None` when its Owned fields are edited as custom fields.
    pub editor: Option<FacetEditorView>,
    /// In the order the section shows them.
    pub fields: Vec<OwnedFieldTemplate>,
}

/// How much a problem a Plugin found matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProblemSeverity {
    /// Probably a mistake; the Entry still saves.
    Warning,
    /// The value can't be kept as it is: the Entry doesn't save until it changes.
    Error,
}

/// What a Plugin says is wrong with a value.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProblemView {
    pub severity: ProblemSeverity,
    /// One sentence saying what to do, shown as is.
    pub message: String,
}

pub(crate) fn facet_template(plugin: &Manifest, facet: &Facet) -> FacetTemplate {
    FacetTemplate {
        plugin: plugin.id.clone(),
        facet: facet.id.clone(),
        title: facet.title.clone(),
        icon: facet.icon.clone(),
        detect: facet.detect.fields.clone(),
        editor: facet.editor.as_ref().map(FacetEditorView::from),
        fields: facet
            .fields
            .iter()
            .map(|field| OwnedFieldTemplate {
                key: field.key.clone(),
                name: field.name.clone(),
                label: field.label.clone(),
                kind: field.kind.into(),
                protected: field.protected,
                placeholder: field.placeholder.clone(),
            })
            .collect(),
    }
}

/// The month `now` is in, in UTC, as `validate` gets it.
pub(crate) fn month_of(now: SystemTime) -> MonthYear {
    // `2026-10-07`
    let date = Timestamp::from_system_time(now).to_date();
    MonthYear {
        year: date[..4].parse().unwrap_or(2000),
        month: date[5..7].parse().unwrap_or(1),
    }
}

fn plugin_error(error: PluginError) -> ApiError {
    match error {
        PluginError::NoFacet(_) => {
            ApiError::new(ErrorKind::NotFound, "That Plugin isn't installed.")
        }
        other => ApiError::new(ErrorKind::Unavailable, other.to_string()),
    }
}

/// Every facet the Plugins contribute, in the order the Plugins were loaded: the editor offers
/// to add the ones with a section of their own.
#[tauri::command]
#[specta::specta]
pub async fn list_facets(core: Core<'_>) -> ApiResult<Vec<FacetTemplate>> {
    Ok(core
        .plugins()
        .manifests()
        .flat_map(|plugin| {
            plugin
                .contributions
                .facets
                .iter()
                .map(move |facet| facet_template(plugin, facet))
        })
        .collect())
}

/// A value as the user typed it into an Owned field, formatted by its Plugin's `format` hook,
/// such as a card number in groups of four.
#[tauri::command]
#[specta::specta]
pub async fn format_owned_field(
    core: Core<'_>,
    plugin: String,
    facet: String,
    key: String,
    value: String,
) -> ApiResult<String> {
    core.plugins()
        .format(&plugin, &facet, &key, &value)
        .map_err(plugin_error)
}

/// What the Plugin's `validate` hook says is wrong with a value for an Owned field, if anything.
/// The hook gets the month it is now, since Plugins have no clock.
#[tauri::command]
#[specta::specta]
pub async fn validate_owned_field(
    core: Core<'_>,
    plugin: String,
    facet: String,
    key: String,
    value: String,
) -> ApiResult<Option<ProblemView>> {
    let problem = core
        .plugins()
        .validate(&plugin, &facet, &key, &value, month_of(SystemTime::now()))
        .map_err(plugin_error)?;
    Ok(problem.map(|problem| ProblemView {
        severity: match problem.severity {
            Severity::Warning => ProblemSeverity::Warning,
            Severity::Error => ProblemSeverity::Error,
        },
        message: problem.message,
    }))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use keefast_plugins::PluginHost;

    use super::*;

    #[test]
    fn the_month_is_the_utc_one() {
        // 2026-10-10 12:00 UTC, and the last second of 2025.
        let month = month_of(UNIX_EPOCH + Duration::from_secs(1_791_633_600));
        assert_eq!((month.year, month.month), (2026, 10));
        let month = month_of(UNIX_EPOCH + Duration::from_secs(1_767_225_599));
        assert_eq!((month.year, month.month), (2025, 12));
    }

    #[test]
    fn the_card_comes_with_its_editor_section() {
        let host = PluginHost::bundled();
        let plugin = host.manifest("keefast.card").unwrap();
        let card = facet_template(plugin, plugin.facet("card").unwrap());
        assert_eq!(card.detect, ["Card number"]);
        assert_eq!(card.editor.unwrap().add, "Add card");
        let protected: Vec<_> = card
            .fields
            .iter()
            .filter(|field| field.protected)
            .map(|field| field.name.as_str())
            .collect();
        assert_eq!(protected, ["Card number", "CVV", "PIN"]);
        assert_eq!(card.fields[2].placeholder.as_deref(), Some("MM/YY"));
    }
}
