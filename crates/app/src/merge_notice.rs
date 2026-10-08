//! Telling the user what a Merge of changes made elsewhere changed.

use keefast_core::MergeSummary;

use crate::strings;

/// One line saying which Vault changes were Merged into and what they changed, such as
/// "Merged changes made elsewhere into personal: 1 Entry updated, 2 Entries added".
pub fn describe(vault: &str, summary: &MergeSummary) -> String {
    let counts = [
        (summary.entries_added, strings::MERGE_ENTRIES_ADDED),
        (summary.entries_updated, strings::MERGE_ENTRIES_UPDATED),
        (summary.entries_moved, strings::MERGE_ENTRIES_MOVED),
        (summary.entries_deleted, strings::MERGE_ENTRIES_DELETED),
        (summary.groups_added, strings::MERGE_GROUPS_ADDED),
        (summary.groups_updated, strings::MERGE_GROUPS_UPDATED),
        (summary.groups_moved, strings::MERGE_GROUPS_MOVED),
        (summary.groups_deleted, strings::MERGE_GROUPS_DELETED),
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
        changes.push(strings::MERGE_OTHER_CHANGES.to_owned());
    }
    format!(
        "{}: {}",
        strings::MERGE_NOTICE.replace("{vault}", vault),
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
