// The pane right of the Entry list: the selected Entry in the shell's detail mode. Later screens
// add their modes here: `edit` (Entry editor A, Tauri UI 3/7) and `history` (Entry history B,
// 4/7). Until they do, every mode shows the Entry.

import { MousePointer2 } from "lucide-react";

import { errorMessage } from "@/api";
import { useEntry } from "@/api/queries";
import { Kbd } from "@/kit";
import { useView } from "@/shell/view";

import { EntryDetail } from "./EntryDetail";

export function DetailArea() {
  const selected = useView()?.entry ?? null;
  const entry = useEntry(selected?.vault, selected?.entry);
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
