// What went wrong talking to keefast, and what the user can do about it, as the popup and the
// command palette say it. They never ask for a master password: keefast is unlocked in keefast
// itself (ADR 0001).
import type { FailureReason } from "./keefast";

export function explain(reason: FailureReason): [title: string, detail?: string] {
  switch (reason) {
    case "not-running":
      return [
        "keefast is not running — start keefast and unlock it.",
        "Browser integration must be on in keefast's settings.",
      ];
    case "locked":
      return ["keefast is locked — unlock it.", "Then try again."];
    case "denied":
      return ["Turned down in keefast."];
    case "failed":
      return ["Something went wrong talking to keefast.", "Try again."];
  }
}
