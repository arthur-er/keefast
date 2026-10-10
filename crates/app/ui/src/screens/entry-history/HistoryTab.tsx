// The Entry detail's History tab (Vault browser C): the versions as a timeline, each saying what
// changed. Opening one shows it in Entry history B, where it is compared and restored.

import { History } from "lucide-react";

import type { EntryDetail } from "@/bindings";
import { useEntryHistory } from "@/api/queries";
import { Button } from "@/kit";
import { formatDate } from "@/lib/format";
import { useShell } from "@/shell/state";

import { allVersions, summarize } from "./model";
import { openHistory } from "./open";

export function HistoryTab({ entry }: { entry: EntryDetail }) {
  const { actions } = useShell();
  const history = useEntryHistory(entry.vault, entry.historyCount > 0 ? entry.id : undefined);
  if (entry.historyCount === 0) {
    return (
      <div className="vb-empty h-[200px]">
        <History />
        <p>This Entry has no earlier versions yet.</p>
        <span className="subtle text-sm">Each edit keeps the previous version here.</span>
      </div>
    );
  }
  const versions = history.data ? allVersions(entry, history.data) : [];
  return (
    <div>
      <div className="vb-sect-head">
        <span>
          {entry.historyCount} earlier version{entry.historyCount === 1 ? "" : "s"}
        </span>
        <span className="spacer" />
        <Button variant="ghost" size="sm" shortcut="Ctrl H" onClick={() => openHistory(actions)}>
          <History />
          Compare versions
        </Button>
      </div>
      {versions.map((version, index) => {
        const when = version.modified !== null ? formatDate(version.modified) : "an unknown date";
        const current = version.key === "current";
        return (
          <div key={String(version.key)} className={current ? "C-ver is-current" : "C-ver"}>
            <span className="node" />
            <div className="min-w-0">
              <div className="what truncate">{current ? "Current version" : summarize(version, versions[index + 1] ?? null).join(" · ")}</div>
              <div className="when">{current ? when : `Version of ${when}`}</div>
            </div>
            {current ? (
              <span />
            ) : (
              <Button variant="ghost" size="sm" onClick={() => openHistory(actions, version.key as number)}>
                Open
              </Button>
            )}
          </div>
        );
      })}
    </div>
  );
}
