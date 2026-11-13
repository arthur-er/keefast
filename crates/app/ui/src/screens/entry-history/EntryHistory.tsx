// Entry history B: the selected Entry's versions (the current one first, then its history, newest
// first) beside the picked one compared field by field with the version before it or the current
// one. Its secrets show only when asked; a version can be made current again. It fills the main
// area right of the sidebar while the shell's detail mode is `history` (DESIGN.md §7).

import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDown, ArrowLeft, ArrowRight, ArrowUp, ChevronRight, CircleDot, Eye, EyeOff, GitCommitVertical, GitCompare, History,
  RotateCcw, Sparkles, X,
} from "lucide-react";

import { api, errorMessage } from "@/api";
import type { EntryDetail, EntryVersion } from "@/bindings";
import { useEntry, useEntryHistory } from "@/api/queries";
import { Button, Segmented, Switch, toast } from "@/kit";
import { cn } from "@/lib/utils";
import { EntryAvatar } from "@/screens/vault-browser/EntryList";
import { useAppCommand } from "@/shell/commands";
import { useKeysActive } from "@/shell/hooks";
import { useShell } from "@/shell/state";
import { MergeNotice } from "@/shell/Toolbar";
import { sameScope, useView } from "@/shell/view";

import {
  allVersions, baseOf, bucket, changedCount, compare, longTime, relativeTime, type Row, type SecretRef, shortTime,
  summarize, type Value, type Version,
} from "./model";
import { takeRequestedVersion } from "./open";

import "./entry-history.css";

const MASK = "••••••••";

/** The page: the selected Entry's history, until the user goes back or the Entry goes. */
export function EntryHistoryPage() {
  const { actions } = useShell();
  const view = useView();
  const selected = view?.entry ?? null;
  const entry = useEntry(selected?.vault, selected?.entry);
  const history = useEntryHistory(selected?.vault, selected?.entry);

  // Another scope picked in the sidebar, the Entry deleted or its Vault locked: back to the list.
  const scope = useRef(view?.scope);
  useEffect(() => {
    if (!view?.entry || entry.error || (scope.current && !sameScope(scope.current, view.scope))) actions.showEntry();
  }, [view, entry.error, actions]);

  const data = entry.data;
  return (
    <div className="eh">
      <HistoryToolbar entry={data} onClose={actions.showEntry} />
      <MergeNotice />
      {data && history.data ? (
        <Versions key={`${data.vault}:${data.id}`} entry={data} history={history.data} />
      ) : history.error ? (
        <div className="vb-empty text-danger">{errorMessage(history.error)}</div>
      ) : null}
    </div>
  );
}

function HistoryToolbar({ entry, onClose }: { entry: EntryDetail | undefined; onClose: () => void }) {
  const group = entry && entry.groupPath.length > 1 ? entry.groupPath[entry.groupPath.length - 1] : null;
  return (
    <div className="toolbar">
      <Button variant="ghost" size="sm" onClick={onClose}>
        <ArrowLeft />
        Back
      </Button>
      {entry && (
        <div className="crumbs min-w-0">
          <span className="truncate">{entry.vaultName}</span>
          {group && (
            <>
              <ChevronRight className="ico-sm subtle" />
              <span className="truncate">{group}</span>
            </>
          )}
          <ChevronRight className="ico-sm subtle" />
          <span className="truncate">{entry.title}</span>
          <ChevronRight className="ico-sm subtle" />
          <b className="truncate">History of {entry.title}</b>
        </div>
      )}
      <span className="spacer" />
      <span className="hint hstack shrink-0 gap-1.5">
        <span className="kbds">
          <kbd>↑</kbd>
          <kbd>↓</kbd>
        </span>
        newer / older
        <span className="w-2" />
        <span className="kbds">
          <kbd>Esc</kbd>
        </span>
        close
      </span>
    </div>
  );
}

type Against = "previous" | "current";

/** Which version is picked: by its key, and when it was saved, which finds it again when a
 * Merge or a restore shifts the history under it. */
interface Picked {
  key: Version["key"];
  modified: number | null;
}

function Versions({ entry, history }: { entry: EntryDetail; history: EntryVersion[] }) {
  const { actions } = useShell();
  const keysActive = useKeysActive();
  const versions = useMemo(() => allVersions(entry, history), [entry, history]);
  const [picked, setPicked] = useState<Picked>(() => {
    const asked = takeRequestedVersion();
    const first = versions.find((version) => version.key === asked) ?? versions[1] ?? versions[0]!;
    return { key: first.key, modified: first.modified };
  });
  const [against, setAgainst] = useState<Against>("previous");
  const [onlyChanges, setOnlyChanges] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [secrets, setSecrets] = useState<ReadonlyMap<string, string>>(new Map());

  const found = versions.findIndex((version) => version.key === picked.key && version.modified === picked.modified);
  const selected = found >= 0 ? found : Math.max(0, versions.findIndex((version) => version.modified === picked.modified && version.key !== "current"));
  // The current version is always there.
  const version = versions[selected] ?? versions[0]!;
  const base = baseOf(versions, selected, against);
  const rows = useMemo(() => compare(version, base), [version, base]);

  const pick = (index: number) => {
    const next = versions[Math.max(0, Math.min(versions.length - 1, index))];
    if (!next || next.key === version.key) return;
    setPicked({ key: next.key, modified: next.modified });
    // A version's secrets show for it alone.
    setRevealed(false);
  };

  // Secrets go when the window is left, as the Entry detail's do.
  useEffect(() => {
    const hide = () => setRevealed(false);
    window.addEventListener("blur", hide);
    return () => window.removeEventListener("blur", hide);
  }, []);

  // Revealing fetches the secrets the rows show, and forgets them when hidden.
  useEffect(() => {
    if (!revealed) {
      setSecrets(new Map());
      return;
    }
    let cancelled = false;
    const refs = secretRefs(rows).filter((ref) => !secrets.has(secretKey(ref)));
    if (refs.length === 0) return;
    void Promise.all(refs.map(async (ref) => [secretKey(ref), await reveal(entry, ref)] as const))
      .then((values) => {
        if (!cancelled) setSecrets((known) => new Map([...known, ...values]));
      })
      .catch((error) => {
        if (cancelled) return;
        setRevealed(false);
        toast(errorMessage(error), { icon: "error" });
      });
    return () => {
      cancelled = true;
    };
    // `secrets` is what this effect fills; refetching on it would loop.
  }, [revealed, rows, entry]);

  const restore = async () => {
    if (!version.saved) return;
    try {
      await api.restoreEntryVersion(entry.vault, entry.id, version.saved.index);
      actions.showEntry();
      toast("Version restored. The one it replaced is kept in the Entry's history.", { icon: History });
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };

  const hasSecrets = secretRefs(rows).length > 0;
  useAppCommand({ id: "olderVersion", title: "Select older version", section: "History", icon: ArrowDown, shortcut: ["ArrowDown"], enabled: selected < versions.length - 1, run: () => pick(selected + 1) }, keysActive);
  useAppCommand({ id: "newerVersion", title: "Select newer version", section: "History", icon: ArrowUp, shortcut: ["ArrowUp"], enabled: selected > 0, run: () => pick(selected - 1) }, keysActive);
  useAppCommand({ id: "restoreVersion", title: "Restore this version", section: "History", icon: RotateCcw, enabled: version.saved !== null, run: () => void restore() }, keysActive);
  useAppCommand(
    { id: "compareVersion", title: against === "previous" ? "Compare with current version" : "Compare with previous version", section: "History", icon: GitCompare, run: () => setAgainst(against === "previous" ? "current" : "previous") },
    keysActive,
  );
  useAppCommand({ id: "revealVersion", title: revealed ? "Hide passwords" : "Show passwords", section: "History", icon: revealed ? EyeOff : Eye, enabled: hasSecrets, run: () => setRevealed(!revealed) }, keysActive);
  useAppCommand({ id: "closeHistory", title: "Close Entry history", section: "History", icon: X, shortcut: ["Escape"], run: actions.showEntry }, keysActive);

  const selectedRow = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    selectedRow.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  if (history.length === 0) {
    return (
      <div className="eh-empty">
        <div>
          <div className="ico">
            <History className="ico-lg" />
          </div>
          <p>This Entry has no earlier versions yet. Each edit keeps the version it replaces here.</p>
          <Button size="sm" onClick={actions.showEntry}>
            Back to {entry.title}
          </Button>
        </div>
      </div>
    );
  }

  const changed = changedCount(rows);
  const fields = `${changed} field${changed === 1 ? "" : "s"}`;
  const note =
    against === "previous"
      ? base
        ? `${fields} changed in this version`
        : "The oldest version kept of this Entry"
      : selected === 0
        ? "This is the Entry as it is now"
        : `Restoring changes ${fields} of the current version`;

  let lastBucket: string | null = null;
  return (
    <div className="eh-split">
      <div className="eh-list">
        <div className="scroll" aria-label="Versions">
          {versions.map((item, index) => {
            const heading = bucket(item);
            const showHeading = heading !== lastBucket;
            lastBucket = heading;
            const older = versions[index + 1] ?? null;
            const oldest = older === null;
            const summary = oldest && !createdThen(entry, item) ? ["Oldest version kept"] : summarize(item, older);
            const Icon = item.key === "current" ? CircleDot : oldest ? Sparkles : GitCommitVertical;
            const isSelected = index === selected;
            return (
              <div key={String(item.key)}>
                {showHeading && <div className="eh-group">{heading}</div>}
                <button
                  ref={isSelected ? selectedRow : undefined}
                  type="button"
                  aria-current={isSelected}
                  className={cn("eh-ver", item.key === "current" && "is-current", isSelected && "is-selected")}
                  onClick={() => pick(index)}
                >
                  <span className="ic">
                    <Icon />
                  </span>
                  <span className="min-w-0">
                    <span className="t block">{item.key === "current" ? "Current version" : `Version of ${shortTime(item.modified)}`}</span>
                    <span className="s block">{summary.join(" · ")}</span>
                  </span>
                </button>
              </div>
            );
          })}
        </div>
      </div>
      <div className="eh-main">
        <div className="eh-top">
          <EntryAvatar entry={entry} size={36} />
          <div className="min-w-0">
            <h1 className="truncate">{selected === 0 ? "Current version" : `Version of ${longTime(version.modified)}`}</h1>
            <div className="sub">
              {selected === 0 ? `Saved ${shortTime(version.modified)}` : relativeTime(version.modified)} · {note}
            </div>
          </div>
          <span className="spacer" />
          {version.saved && (
            <Button variant="primary" size="sm" onClick={() => void restore()}>
              <RotateCcw />
              Restore this version
            </Button>
          )}
        </div>
        <div className="eh-bar">
          <span>Compare with</span>
          <Segmented<Against>
            label="Compare with"
            value={against}
            onValueChange={setAgainst}
            options={[
              { value: "previous", label: "Previous version" },
              { value: "current", label: "Current version" },
            ]}
          />
          <span className="spacer" />
          <label className="hstack cursor-default gap-2">
            <Switch checked={onlyChanges} onCheckedChange={setOnlyChanges} label="Only changes" />
            Only changes
          </label>
          <span className="vdivider h-4 self-center" />
          <Button variant="ghost" size="sm" disabled={!hasSecrets} onClick={() => setRevealed(!revealed)}>
            {revealed ? <EyeOff /> : <Eye />}
            {revealed ? "Hide" : "Show"} passwords
          </Button>
        </div>
        <div className="eh-body">
          <div className="eh-table">
            {rows
              .filter((row) => !onlyChanges || row.change)
              .map((row) => (
                <div key={row.key} className={cn("eh-row", row.change && "is-changed")}>
                  <span className="k" title={row.label}>
                    {row.label}
                  </span>
                  <span className="v">
                    <RowValue row={row} secrets={revealed ? secrets : null} />
                  </span>
                </div>
              ))}
          </div>
        </div>
      </div>
    </div>
  );
}

/** Whether the oldest version kept is the Entry as it was created (KeePass trims old ones). */
function createdThen(entry: EntryDetail, oldest: Version): boolean {
  return entry.created === null || oldest.modified === null || Math.abs(oldest.modified - entry.created) < 60;
}

function RowValue({ row, secrets }: { row: Row; secrets: ReadonlyMap<string, string> | null }) {
  if (!row.change) return <ValueView value={row.value} secrets={secrets} multiline={row.key === "notes"} />;
  const { from, to } = row.change;
  const fromView = <ValueView value={from} secrets={secrets} multiline={row.key === "notes"} />;
  const toView = <ValueView value={to} secrets={secrets} multiline={row.key === "notes"} />;
  // A value added or removed shows alone, but lists (tags, attachments) say they became empty.
  const list = row.key === "tags" || row.key === "attachments";
  if (from.kind === "empty") return <span className="ins">{toView}</span>;
  if (to.kind === "empty" && !list) return <span className="del">{fromView}</span>;
  if (row.key === "notes") {
    return (
      <span className="stack">
        <span className="del">{fromView}</span>
        <span className="ins">{toView}</span>
      </span>
    );
  }
  return (
    <>
      <span className="del">{fromView}</span>
      <span className="arrow">
        <ArrowRight className="ico-sm" />
      </span>
      <span className="ins">{toView}</span>
    </>
  );
}

function ValueView({ value, secrets, multiline }: { value: Value; secrets: ReadonlyMap<string, string> | null; multiline?: boolean }) {
  switch (value.kind) {
    case "empty":
      return <span className="subtle">Empty</span>;
    case "text":
      return <span className={cn("selectable", value.mono && "val-mono", multiline && "notes")}>{value.text}</span>;
    case "secret": {
      const shown = secrets?.get(secretKey(value.ref));
      return <span className={cn("val-mono", shown !== undefined && "selectable")}>{shown ?? MASK}</span>;
    }
  }
}

const secretKey = (ref: SecretRef) => `${ref.version}:${ref.field}`;

function secretRefs(rows: readonly Row[]): SecretRef[] {
  const refs: SecretRef[] = [];
  for (const row of rows) {
    for (const value of row.change ? [row.change.from, row.change.to] : [row.value]) {
      if (value.kind === "secret") refs.push(value.ref);
    }
  }
  return refs;
}

function reveal(entry: EntryDetail, ref: SecretRef): Promise<string> {
  if (ref.version === "current") {
    if (ref.field === "password") return api.revealPassword(entry.vault, entry.id);
    // By name: the Entry's fields may have moved since this snapshot of it.
    const field = entry.customFields[ref.field];
    return field ? api.revealCustomField(entry.vault, entry.id, field.name) : Promise.reject(new Error("That field is no longer in the Entry."));
  }
  return ref.field === "password"
    ? api.revealVersionPassword(entry.vault, entry.id, ref.version)
    : api.revealVersionCustomField(entry.vault, entry.id, ref.version, ref.field);
}
