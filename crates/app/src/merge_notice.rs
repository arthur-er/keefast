//! Telling the user what a Merge of changes made elsewhere changed.

use keefast_core::MergeSummary;

/// One line saying which Vault changes were Merged into and what they changed, such as
/// "Merged changes made elsewhere into personal: 1 Entry updated, 2 Entries added".
pub fn describe(vault: &str, summary: &MergeSummary) -> String {
    let counts = [
        (
            summary.entries_added,
            ["{n} Entry added", "{n} Entries added"],
        ),
        (
            summary.entries_updated,
            ["{n} Entry updated", "{n} Entries updated"],
        ),
        (
            summary.entries_moved,
            ["{n} Entry moved", "{n} Entries moved"],
        ),
        (
            summary.entries_deleted,
            ["{n} Entry deleted", "{n} Entries deleted"],
        ),
        (
            summary.groups_added,
            ["{n} Group added", "{n} Groups added"],
        ),
        (
            summary.groups_updated,
            [
                "{n} Group renamed or changed",
                "{n} Groups renamed or changed",
            ],
        ),
        (
            summary.groups_moved,
            ["{n} Group moved", "{n} Groups moved"],
        ),
        (
            summary.groups_deleted,
            ["{n} Group deleted", "{n} Groups deleted"],
        ),
    ];
    let mut changes: Vec<String> = counts
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, [one, many])| {
            let text = if count == 1 { one } else { many };
            text.replace("{n}", &count.to_string())
        })
        .collect();
    if changes.is_empty() {
        changes.push("history and settings updated".to_owned());
    }
    format!(
        "Merged changes made elsewhere into {vault}: {}",
        changes.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_what_changed_in_which_vault() {
        let mut summary = MergeSummary::default();
        summary.entries_updated = 1;
        summary.entries_added = 2;
        summary.groups_deleted = 1;

        assert_eq!(
            describe("personal", &summary),
            "Merged changes made elsewhere into personal: 2 Entries added, 1 Entry updated, \
             1 Group deleted"
        );
    }

    #[test]
    fn a_merge_of_history_only_still_says_so() {
        assert_eq!(
            describe("work", &MergeSummary::default()),
            "Merged changes made elsewhere into work: history and settings updated"
        );
    }
}
