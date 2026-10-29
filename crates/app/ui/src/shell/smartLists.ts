// The Smart lists under All Entries in the sidebar: a Vault's Entries outside the Recycle Bin
// with TOTP, changed in the last 30 days (newest first), or expired. The backend lists them;
// this names them and, for the command palette and the mock backend, says which Entries are on
// one as the backend does.

import { Clock, type LucideIcon, Timer, TriangleAlert } from "lucide-react";

import type { EntrySummary, SmartList, SmartListCounts } from "@/bindings";

export const SMART_LISTS: readonly { list: SmartList; label: string; icon: LucideIcon; count: keyof SmartListCounts }[] = [
  { list: "withTotp", label: "With TOTP", icon: Timer, count: "withTotp" },
  { list: "recentlyModified", label: "Recently modified", icon: Clock, count: "recentlyModified" },
  { list: "expired", label: "Expired", icon: TriangleAlert, count: "expired" },
];

/** How long ago an Entry may have changed to be on Recently modified, as the backend has it. */
export const RECENTLY_MODIFIED_SECONDS = 30 * 24 * 60 * 60;

export function smartListName(list: SmartList): string {
  return SMART_LISTS.find((smart) => smart.list === list)!.label;
}

/** Whether the Entry, outside the Recycle Bin, is on the list at `now` (seconds since 1970). */
export function onSmartList(
  list: SmartList,
  entry: Pick<EntrySummary, "hasTotp" | "modified" | "expired">,
  now: number,
): boolean {
  switch (list) {
    case "withTotp":
      return entry.hasTotp;
    case "recentlyModified":
      return entry.modified !== null && entry.modified >= now - RECENTLY_MODIFIED_SECONDS;
    case "expired":
      return entry.expired;
  }
}
