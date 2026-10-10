// The pane right of the Entry list: the selected Entry in the shell's detail mode. `edit` is
// Entry editor A; `history` (Entry history B, 4/7) shows the Entry until it arrives.

import { MousePointer2, SquarePen } from "lucide-react";

import { errorMessage } from "@/api";
import { useEntry } from "@/api/queries";
import { Kbd } from "@/kit";
import { SHORTCUTS } from "@/lib/shortcuts";
import { EntryEditorHost } from "@/screens/entry-editor/EntryEditor";
import { startNewEntry } from "@/screens/entry-editor/session";
import { useAppCommand } from "@/shell/commands";
import { useKeysActive } from "@/shell/hooks";
import { useShell } from "@/shell/state";
import { useView } from "@/shell/view";

import { EntryDetail } from "./EntryDetail";

export function DetailArea() {
  const { state, actions } = useShell();
  const view = useView();
  const selected = view?.entry ?? null;
  const entry = useEntry(selected?.vault, selected?.entry);
  useAppCommand(
    { id: "newEntry", title: "New Entry", section: "Entry", icon: SquarePen, shortcut: SHORTCUTS.newEntry, run: () => void startNewEntry(view, actions) },
    useKeysActive(),
  );
  if (state.detail === "edit") {
    return (
      <section className="vb-detail" aria-label="Edit Entry">
        <EntryEditorHost />
      </section>
    );
  }
  return (
    <section className="vb-detail" aria-label="Entry">
      {!selected ? (
        <NoSelection />
      ) : entry.error ? (
        <div className="vb-empty text-danger">{errorMessage(entry.error)}</div>
      ) : entry.data ? (
        // Keyed by Entry: a revealed password, the tab and "Copied" don't carry over.
        <EntryDetail key={`${entry.data.vault}:${entry.data.id}`} entry={entry.data} />
      ) : null}
    </section>
  );
}

function NoSelection() {
  return (
    <div className="vb-empty">
      <MousePointer2 />
      <p>Select an Entry</p>
      <span className="subtle hstack gap-1 text-sm">
        <Kbd combo="ArrowUp" /> <Kbd combo="ArrowDown" /> to move
      </span>
    </div>
  );
}
