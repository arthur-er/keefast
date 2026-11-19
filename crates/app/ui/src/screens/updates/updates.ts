// Updates (ADR 0014): what the update notice and General Settings' Updates rows say, from the
// backend's `UpdateState` and whether the user has agreed to automatic checks.

import type { AvailableUpdate, UpdateState, UpdateStatus } from "@/bindings";

export const ASK_UPDATES = "Check for keefast updates once a day?";
export const ASK_UPDATES_DESC = "keefast asks GitHub for its latest release; GitHub sees only your IP address.";
export const CHECK_UPDATES_DESC = "Once a day, ask GitHub whether there is a newer keefast";

/** What the update notice over the Vault browser shows, if anything. */
export type UpdateNotice =
  | { kind: "ask" }
  | { kind: "available"; update: AvailableUpdate; installs: boolean }
  | { kind: "downloading"; update: AvailableUpdate; progress: number | null }
  | { kind: "failed"; update: AvailableUpdate; message: string };

/**
 * The notice to show: an update found, downloading or failing to install comes first; then, until
 * the user answers, whether to check automatically. `dismissed` is the update versions the user
 * closed the notice of. A check failing with no update found says nothing here: it shows in
 * Settings, as an automatic check runs unattended.
 */
export function updateNotice(state: UpdateState | undefined, checkForUpdates: boolean | null | undefined, dismissed: ReadonlySet<string>): UpdateNotice | null {
  const status = state?.status;
  if (status?.kind === "downloading") return { kind: "downloading", update: status.update, progress: status.progress };
  if (status?.kind === "available" && !dismissed.has(status.update.version)) {
    return { kind: "available", update: status.update, installs: state!.installsUpdates };
  }
  if (status?.kind === "failed" && status.update && !dismissed.has(status.update.version)) {
    return { kind: "failed", update: status.update, message: status.message };
  }
  return checkForUpdates === null ? { kind: "ask" } : null;
}

/** The update found, if any: to install, download or retry. */
export function foundUpdate(status: UpdateStatus | undefined): AvailableUpdate | null {
  switch (status?.kind) {
    case "available":
    case "downloading":
      return status.update;
    case "failed":
      return status.update;
    default:
      return null;
  }
}

/** One line on where updating stands, for Settings. */
export function updateStatusText(state: UpdateState, now: number = Date.now()): string {
  const status = state.status;
  switch (status.kind) {
    case "unchecked":
      return `keefast ${state.currentVersion}`;
    case "checking":
      return "Checking for updates…";
    case "upToDate":
      return `keefast ${state.currentVersion} is up to date · checked ${checkedWhen(status.checkedAt * 1000, now)}`;
    case "available":
      return state.installsUpdates
        ? `keefast ${status.update.version} is available`
        : `keefast ${status.update.version} is available: download it from its release page`;
    case "downloading":
      return downloadingText(status.update, status.progress);
    case "failed":
      return status.message;
  }
}

export function downloadingText(update: AvailableUpdate, progress: number | null): string {
  const percent = progress === null ? "" : ` ${Math.round(progress * 100)}%`;
  return `Downloading keefast ${update.version}…${percent}`;
}

/** "just now", "5 min ago", or the time of day. */
function checkedWhen(at: number, now: number): string {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  return new Date(at).toLocaleTimeString("en", { hour: "numeric", minute: "2-digit" });
}
