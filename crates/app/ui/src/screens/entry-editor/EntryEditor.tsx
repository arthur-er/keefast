// Entry editor A: the selected Entry (or a new one) edited in place in Vault browser C's detail
// area. "Done" (Ctrl S) saves every change at once; "Cancel" (Esc) drops them. Attachments,
// custom icons and a saved Entry's TOTP apply as they are made: they have commands of their own.
// Plugins' facets with an editor section (a Card) are sections after Login, added beside
// "+ Field" and saved with the Entry.
//
// The editor stays on what it edits while the list's selection moves: moving away asks before
// dropping unsaved changes, and otherwise just shows the newly selected Entry.
//
// A Merge can change the Entry while it is being edited. Saving is then refused, so the change
// made elsewhere is never overwritten unseen: the user reloads the Entry, or keeps their edits,
// which saves them over it and leaves the Merged version in History.

import { useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  ArrowUpRight, Check, ChevronRight, CircleAlert, Copy, Eye, EyeOff, Folder, History, Pencil, Plus, Timer, WandSparkles, X,
} from "lucide-react";

import { api, ApiError, errorMessage } from "@/api";
import type { EntryDetail, EntryKey, StrengthView, TotpSetup } from "@/bindings";
import { keys, useEntry, useFacets, useGroups, useWorkspace } from "@/api/queries";
import { Badge, Button, GroupIcon, IconButton, Kbd, StrengthMeter, Switch, toast } from "@/kit";
import { formatDate } from "@/lib/format";
import { SHORTCUTS } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { GeneratorPopover, GeneratorTrigger } from "@/screens/password-generator/GeneratorPopover";
import {
  addableFacets, addFacet, type FacetDraft, type OwnedRow, sameFacet, withChange, withFormatted, withRevealed,
} from "@/screens/plugins/editing";
import { FacetEditor, rowsWithErrors, useFacetChecks } from "@/screens/plugins/FacetEditor";
import { LiveTotp, TotpSetupPanel } from "@/screens/totp/TotpSetup";
import { copyField, openUrl } from "@/screens/vault-browser/actions";
import { useAppCommand } from "@/shell/commands";
import { useKeysActive } from "@/shell/hooks";
import { findGroup, groupPath } from "@/shell/Sidebar";
import { useTagColor } from "@/shell/tagColors";
import { useShell } from "@/shell/state";
import { showView, useView } from "@/shell/view";

import { Attachments } from "./Attachments";
import {
  type Draft, draftExpired, draftOf, ERROR_TEXT, type FieldRow, hiddenFields, isDirty, newDraft, problems, rowKey,
  savedIndex, toInput,
} from "./draft";
import { IconPicker } from "./IconPicker";
import { ColorChip, ExpiryChip, TagInput } from "./Props";
import { type NewEntryTarget, setEntryWithChanges, takeNewEntryRequest, useNewEntryRequest } from "./session";
import "./entry-editor.css";

type Session = { id: number; kind: "existing"; key: EntryKey } | { id: number; kind: "new"; target: NewEntryTarget };

let sessions = 0;
const sameKey = (a: EntryKey | null, b: EntryKey | null) =>
  a === b || (a !== null && b !== null && a.vault === b.vault && a.entry === b.entry);

/** The detail area in `edit` mode. */
export function EntryEditorHost() {
  const { actions } = useShell();
  const view = useView();
  const selected = view?.entry ?? null;
  const request = useNewEntryRequest();
  const [session, setSession] = useState<Session | null>(() =>
    request ? { id: ++sessions, kind: "new", target: request } : selected ? { id: ++sessions, kind: "existing", key: selected } : null,
  );
  const dirty = useRef(false);
  const title = useRef("");
  const anchor = useRef(selected);

  /** Runs `proceed`, after asking when there are unsaved changes to drop. */
  const leave = (proceed: () => void) => {
    if (!dirty.current) return proceed();
    actions.confirm({
      title: title.current ? `Discard your changes to “${title.current}”?` : "Discard the new Entry?",
      description: "They haven't been saved. Keep editing to save them with Done.",
      confirmLabel: "Discard",
      danger: true,
      onConfirm: () => {
        dirty.current = false;
        proceed();
      },
    });
  };

  // A new Entry asked for: taken at once, edited after asking about changes to this one.
  useEffect(() => {
    if (!request) return;
    const taken = takeNewEntryRequest();
    if (!taken || (session?.kind === "new" && session.target === taken)) return;
    leave(() => {
      anchor.current = selected;
      setSession({ id: ++sessions, kind: "new", target: taken });
    });
  }, [request]);

  // The list's selection moved away: show the Entry selected, unless the user keeps editing.
  useEffect(() => {
    if (sameKey(selected, anchor.current)) return;
    anchor.current = selected;
    leave(() => actions.showEntry());
  }, [selected?.vault, selected?.entry]);

  useEffect(() => {
    if (!session) actions.showEntry();
  }, [session, actions]);
  if (!session) return null;

  const done = (key: EntryKey | null) => {
    dirty.current = false;
    if (key && view && !sameKey(view.entry, key)) {
      anchor.current = key;
      showView({ scope: view.scope, entry: key });
    }
    actions.showEntry();
  };

  return session.kind === "existing" ? (
    <ExistingEditor key={session.id} entryKey={session.key} dirty={dirty} title={title} onDone={done} leave={leave} />
  ) : (
    <EditorForm key={session.id} entry={null} target={session.target} dirty={dirty} title={title} onDone={done} leave={leave} />
  );
}

interface FormHooks {
  dirty: React.RefObject<boolean>;
  title: React.RefObject<string>;
  onDone: (key: EntryKey | null) => void;
  leave: (proceed: () => void) => void;
}

function ExistingEditor({ entryKey, ...hooks }: { entryKey: EntryKey } & FormHooks) {
  const { actions } = useShell();
  const client = useQueryClient();
  const entry = useEntry(entryKey.vault, entryKey.entry);
  // Bumped to reload: the form starts again from the Entry as it is now.
  const [loads, setLoads] = useState(0);
  useEffect(() => {
    if (!entry.error) return;
    toast(errorMessage(entry.error), { icon: "error" });
    actions.showEntry();
  }, [entry.error, actions]);
  if (!entry.data) return null;
  const reload = async () => {
    await client.refetchQueries({ queryKey: keys.entry(entryKey.vault, entryKey.entry) });
    hooks.dirty.current = false;
    setLoads((count) => count + 1);
  };
  return <EditorForm key={loads} entry={entry.data} target={null} onReload={reload} {...hooks} />;
}

function EditorForm({
  entry,
  target,
  dirty,
  title,
  onDone,
  leave,
  onReload,
}: { entry: EntryDetail | null; target: NewEntryTarget | null; onReload?: () => Promise<void> } & FormHooks) {
  const { actions } = useShell();
  const client = useQueryClient();
  const keysActive = useKeysActive();
  const snapshot = useWorkspace().data;
  const vault = entry?.vault ?? target!.vault;
  const groups = useGroups(vault).data;
  const colorOf = useTagColor();
  const [saved] = useState<Draft>(() => (entry ? draftOf(entry) : newDraft({ tags: target?.tags, color: target?.color })));
  const [draft, setDraft] = useState<Draft>(saved);
  const [stagedTotp, setStagedTotp] = useState<TotpSetup | null>(null);
  const [totpOpen, setTotpOpen] = useState(false);
  const [generatorOpen, setGeneratorOpen] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [shownFields, setShownFields] = useState<ReadonlySet<number>>(new Set());
  const [tried, setTried] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The Entry's last-modified time as the editor last saw it: when it opened, or after a change
  // the editor applied at once. A save is refused if the Entry changed since in any other way.
  const seen = useRef(entry?.modified ?? null);
  const [changedElsewhere, setChangedElsewhere] = useState(false);
  const titleField = useRef<HTMLInputElement>(null);
  const doc = useRef<HTMLDivElement>(null);

  const changed = isDirty(saved, draft) || stagedTotp !== null;
  dirty.current = changed;
  title.current = entry?.title ?? "";
  // The Entry list marks the Entry while it has unsaved changes, until the editor closes.
  useEffect(() => {
    setEntryWithChanges(changed && entry ? { vault: entry.vault, entry: entry.id } : null);
  }, [changed, entry?.vault, entry?.id]);
  useEffect(() => () => setEntryWithChanges(null), []);
  const hidden = hiddenFields(entry, draft).map((field) => field.name);
  const found = tried ? problems(draft, hidden) : null;
  const update = (change: Partial<Draft>) => {
    setDraft((current) => ({ ...current, ...change }));
    setSaveError(null);
  };
  const updateRow = (key: number, change: Partial<FieldRow>) =>
    setDraft((current) => ({ ...current, fields: current.fields.map((row) => (row.key === key ? { ...row, ...change } : row)) }));
  const updateFacets = (change: (facets: FacetDraft[]) => FacetDraft[]) => {
    setDraft((current) => ({ ...current, facets: change(current.facets) }));
    setSaveError(null);
  };
  const templates = useFacets().data ?? [];
  const checks = useFacetChecks(draft.facets);
  const ownedErrors = rowsWithErrors(draft.facets, checks);

  useEffect(() => {
    // A new Entry starts at its title; an Entry being edited, where the user clicks.
    if (!entry) titleField.current?.focus();
    // Revealed secrets go when the window is left.
    const hide = () => {
      setShowPassword(false);
      setShownFields(new Set());
    };
    window.addEventListener("blur", hide);
    return () => window.removeEventListener("blur", hide);
  }, []);

  /** Fills the password in from the backend, the first time the user wants it. */
  const revealPassword = async () => {
    if (draft.password !== null || !entry) return;
    try {
      const password = await api.revealPassword(entry.vault, entry.id);
      setDraft((current) => (current.password === null ? { ...current, password, savedPassword: password } : current));
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  const revealField = async (row: FieldRow) => {
    if (row.value !== null || !entry || row.original === null || savedIndex(entry, row.original) === null) return;
    try {
      const value = await api.revealCustomField(entry.vault, entry.id, row.original);
      setDraft((current) => ({
        ...current,
        fields: current.fields.map((r) => (r.key === row.key && r.value === null ? { ...r, value, savedValue: value } : r)),
      }));
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };

  /** Fills an Owned field in from the backend, the first time the user wants it. */
  const revealOwned = async (row: OwnedRow) => {
    if (row.value !== null || !entry || row.original === null || savedIndex(entry, row.original) === null) return;
    try {
      const value = await api.revealCustomField(entry.vault, entry.id, row.original);
      updateFacets((facets) => withRevealed(facets, row.key, value));
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  /** Runs a change that applies to the Entry at once (attachments, TOTP), and moves `seen` on
   * past it, unless the Entry had changed elsewhere before it. */
  const applyNow = async <T,>(run: () => Promise<T>): Promise<T> => {
    if (!entry) return run();
    const before = (await api.getEntry(entry.vault, entry.id)).modified;
    const result = await run();
    if (before === seen.current) seen.current = (await api.getEntry(entry.vault, entry.id)).modified;
    return result;
  };
  const showRow = (key: number, show: boolean) =>
    setShownFields((current) => {
      const next = new Set(current);
      if (show) next.add(key);
      else next.delete(key);
      return next;
    });

  /** Saves the draft. `keep`: the user chose to keep their edits over a change Merged in. */
  const save = async (keep = false) => {
    setTried(true);
    const wrong = problems(draft, hidden);
    if (wrong || ownedErrors.length > 0) {
      requestAnimationFrame(() => {
        const bad = doc.current?.querySelector<HTMLElement>(".invalid input, input.invalid, .is-invalid");
        bad?.scrollIntoView({ block: "center", behavior: "smooth" });
        bad?.focus({ preventScroll: true });
      });
      return;
    }
    if (!changed && entry) return onDone(null);
    if (busy) return;
    setBusy(true);
    try {
      if (entry) {
        // The Entry as it is now: its attachments or TOTP may have changed while editing.
        const latest = await api.getEntry(entry.vault, entry.id);
        if (!keep && latest.modified !== seen.current) {
          setChangedElsewhere(true);
          return;
        }
        // The backend refuses it too if the Entry changes between reading it and saving.
        await api.updateEntry(entry.vault, entry.id, toInput(draft, latest), latest.modified);
        // The detail shows the Entry as saved, not as it was a moment ago.
        await client.refetchQueries({ queryKey: keys.entry(entry.vault, entry.id) });
        toast(<>{draft.title || "(untitled)"} updated</>, { icon: "check" });
        onDone(null);
      } else {
        const id = await api.createEntry(target!.vault, target!.group, toInput(draft, null));
        if (stagedTotp) {
          await api.setTotp(target!.vault, id, stagedTotp).catch((error) => toast(errorMessage(error), { icon: "error" }));
        }
        // The list has it before it's selected: a list without it would select its first Entry.
        await client.refetchQueries({ queryKey: ["entries"], type: "active" });
        toast(<>{draft.title || "(untitled)"} created</>, { icon: "check" });
        onDone({ vault: target!.vault, entry: id });
      }
    } catch (error) {
      if (error instanceof ApiError && error.kind === "entryChanged") setChangedElsewhere(true);
      else setSaveError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };
  const reload = async () => {
    if (!onReload || !entry) return;
    await onReload();
    toast(<>Reloaded {entry.title}</>, { icon: "check" });
  };
  const cancel = () => {
    if (changed) toast("Changes discarded", { icon: X });
    dirty.current = false;
    actions.showEntry();
  };

  // Ctrl S (or Ctrl Enter) is Done here, before the app saves every Vault with it; Esc cancels
  // once popovers and fields have had it.
  const latest = useRef({ save, cancel });
  latest.current = { save, cancel };
  useEffect(() => {
    if (!keysActive) return;
    const done = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && !event.altKey && (event.key.toLowerCase() === "s" || event.key === "Enter")) {
        event.preventDefault();
        void latest.current.save();
      }
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (event.target instanceof Element && event.target.closest('[role="menu"], [role="dialog"], .popover')) return;
      event.preventDefault();
      latest.current.cancel();
    };
    window.addEventListener("keydown", done, true);
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("keydown", done, true);
      window.removeEventListener("keydown", escape);
    };
  }, [keysActive]);
  useAppCommand({ id: "entryEditorDone", title: "Save the Entry", section: "Entry", icon: Check, run: () => void latest.current.save() });
  useAppCommand({ id: "entryEditorCancel", title: "Cancel editing", section: "Entry", icon: X, run: () => latest.current.cancel() });
  useAppCommand(
    { id: "generatePassword", title: "Generate a password", section: "Entry", icon: WandSparkles, shortcut: SHORTCUTS.generatePassword, run: () => setGeneratorOpen((open) => !open) },
    keysActive,
  );
  const strength = usePasswordStrength(entry, draft.password === draft.savedPassword ? null : draft.password);

  const vaultName = entry?.vaultName ?? snapshot?.vaults.find((candidate) => candidate.id === vault)?.name ?? "";
  const place = useMemo(() => {
    if (entry) return entry.groupPath.slice(1);
    if (!groups || !target) return [];
    return groupPath(groups, target.group)
      .slice(1)
      .map((group) => (group.isRecycleBin ? "Recycle Bin" : group.name));
  }, [entry, groups, target]);
  // The Group the Entry is in, or goes into.
  const home = useMemo(() => {
    const id = entry?.group ?? target?.group;
    return groups && id ? findGroup(groups, (group) => group.id === id) : null;
  }, [entry, groups, target]);
  const passwordKept = draft.password === null || draft.password === draft.savedPassword;
  const totpReady = entry?.totp?.status === "ready";
  const errors = [
    ...(found?.errors.map((error) => ERROR_TEXT[error]) ?? []),
    ...(tried && ownedErrors.length > 0 ? ["Fix the fields marked in red to save."] : []),
    ...(saveError ? [saveError] : []),
  ];

  return (
    <div className="ee">
      <div className="ee-bar">
        <div className="crumbs">
          <span className="truncate">{vaultName}</span>
          {place.map((part, index) => (
            <span key={index} className="contents">
              <ChevronRight />
              <span className="truncate">{part}</span>
            </span>
          ))}
          <ChevronRight />
          <b className="truncate">{entry ? draft.title || "(untitled)" : "New Entry"}</b>
        </div>
        <Badge tone="accent" className="ml-1">
          <Pencil className="ico-sm" />
          Editing
        </Badge>
        <span className="spacer" />
        <span className={cn("ee-dirty", changed && "is-on")} role="status">
          <i />
          {changed ? "Unsaved changes" : "No changes"}
        </span>
        <span className="vdivider mx-1 h-4 self-center" />
        <Button variant="ghost" onClick={cancel} shortcut="Escape">
          Cancel
        </Button>
        <Button variant="primary" onClick={() => void save()} disabled={busy} shortcut="Ctrl S">
          {entry ? "Done" : "Create"}
        </Button>
      </div>
      <div className="ee-pane">
        <div className="ee-doc" ref={doc}>
          <div className="ee-title-row">
            <IconPicker
              icon={draft.icon}
              vault={vault}
              // The site's favicon shows while the icon is the Entry's own.
              faviconUrl={JSON.stringify(draft.icon) === JSON.stringify(saved.icon) ? (entry?.faviconUrl ?? null) : null}
              onChange={(icon) => update({ icon })}
            />
            <input
              ref={titleField}
              className="ee-title"
              aria-label="Title"
              value={draft.title}
              placeholder="(untitled)"
              onChange={(event) => update({ title: event.target.value })}
            />
          </div>
          <div className="ee-props">
            <ColorChip color={draft.color} onChange={(color) => update({ color })} />
            <ExpiryChip
              expires={draft.expires}
              expiry={draft.expiry}
              expired={draftExpired(draft)}
              invalid={!!found?.expiry}
              onChange={(change) => update(change)}
            />
            <span className="vdivider mx-1 h-4 self-center" />
            <TagInput
              tags={draft.tags}
              known={snapshot?.tags ?? []}
              colorOf={(tag) => colorOf(vault, tag)}
              onChange={(tags) => update({ tags })}
            />
          </div>
          {changedElsewhere && (
            <div className="ee-changed" role="alert">
              <CircleAlert />
              <div className="msg">
                <b>This Entry changed while you were editing it</b>
                <span>Changes made elsewhere were Merged into it. Reload it to see them, or keep your edits to save them over it: the Merged version stays in History.</span>
              </div>
              <Button variant="ghost" size="sm" disabled={busy} onClick={() => void reload()}>
                Reload
              </Button>
              <Button size="sm" disabled={busy} onClick={() => void save(true)}>
                Keep my edits
              </Button>
            </div>
          )}
          {errors.length > 0 && (
            <div className="ee-errors" role="alert">
              {errors.map((error) => (
                <div key={error} className="ee-err">
                  <CircleAlert />
                  <span>{error}</span>
                </div>
              ))}
            </div>
          )}

          <div className="ee-sec">
            <div className="ee-sec-head">
              <h3>Login</h3>
            </div>
            <div className="ee-kv">
              <span className="k">Username</span>
              <div className="gi-group">
                <input aria-label="Username" value={draft.username} spellCheck={false} autoComplete="off" onChange={(event) => update({ username: event.target.value })} />
                <IconButton
                  size="sm"
                  label={entry && draft.username === saved.username ? "Copy · Ctrl B" : "Copy the saved username: save first"}
                  disabled={!entry || !entry.username || draft.username !== saved.username}
                  onClick={() => entry && void copyField(entry.vault, entry.id, { kind: "username" }, "Username")}
                >
                  <Copy />
                </IconButton>
              </div>
            </div>
            <div className="ee-kv">
              <span className="k">Password</span>
              <GeneratorPopover
                open={generatorOpen}
                onOpenChange={setGeneratorOpen}
                onUse={(password) => {
                  update({ password });
                  setShowPassword(true);
                  toast(<>Generated password filled in<span className="subtle"> · {entry ? "Done" : "Create"} saves it</span></>, { icon: WandSparkles });
                }}
                onOpenSettings={() => leave(() => actions.openSettings("generator"))}
              >
                <div className={cn("gi-group", generatorOpen && "is-open")}>
                  <input
                    aria-label="Password"
                    className={cn("mono", draft.password === null && "kept")}
                    type={showPassword ? "text" : "password"}
                    value={draft.password ?? ""}
                    placeholder={draft.password === null ? "••••••••••••" : entry ? "None" : "Type or generate one"}
                    spellCheck={false}
                    autoComplete="new-password"
                    onFocus={() => void revealPassword()}
                    onChange={(event) => update({ password: event.target.value })}
                  />
                  <IconButton
                    size="sm"
                    label={showPassword ? "Hide" : "Show"}
                    disabled={draft.password === "" && saved.password === ""}
                    onClick={async () => {
                      if (!showPassword) await revealPassword();
                      setShowPassword(!showPassword);
                    }}
                  >
                    {showPassword ? <EyeOff /> : <Eye />}
                  </IconButton>
                  <IconButton
                    size="sm"
                    label={entry && passwordKept ? "Copy · Ctrl C" : "Copy the saved password: save first"}
                    disabled={!entry?.hasPassword || !passwordKept}
                    onClick={() => entry && void copyField(entry.vault, entry.id, { kind: "password" }, "Password")}
                  >
                    <Copy />
                  </IconButton>
                  <GeneratorTrigger asChild>
                    <Button variant="secondary" size="sm" aria-expanded={generatorOpen} title="Generate a password · Ctrl G">
                      <WandSparkles /> Generate
                    </Button>
                  </GeneratorTrigger>
                </div>
              </GeneratorPopover>
            </div>
            {strength && (
              <div className="ee-kv ee-strength">
                <span />
                <StrengthMeter strength={strength} />
              </div>
            )}
            <div className="ee-kv">
              <span className="k">URL</span>
              <div className="gi-group">
                <input aria-label="URL" value={draft.url} placeholder="https://" spellCheck={false} autoComplete="off" onChange={(event) => update({ url: event.target.value })} />
                <IconButton
                  size="sm"
                  label={entry && draft.url === saved.url ? "Open" : "Open the saved URL: save first"}
                  disabled={!entry || !entry.url || draft.url !== saved.url}
                  onClick={() => entry && openUrl(entry.vault, entry.id)}
                >
                  <ArrowUpRight />
                </IconButton>
              </div>
            </div>
            <div className="ee-kv">
              <span className="k">TOTP</span>
              {totpReady || entry?.totp?.status === "unreadable" || stagedTotp ? (
                <div className={cn("tt-row", totpOpen && "is-open")}>
                  {totpReady && entry ? (
                    <LiveTotp vault={entry.vault} entry={entry.id} />
                  ) : entry?.totp?.status === "unreadable" ? (
                    <span className="subtle truncate text-sm" title={entry.totp.reason}>
                      Can't be read: {entry.totp.reason}
                    </span>
                  ) : (
                    <span className="subtle text-sm">Set up · saved with the Entry</span>
                  )}
                  <span className="spacer" />
                  <Button variant="ghost" size="sm" className={cn(totpOpen && "is-on")} aria-expanded={totpOpen} onClick={() => setTotpOpen(!totpOpen)}>
                    TOTP…
                  </Button>
                </div>
              ) : (
                <div>
                  <button type="button" className="tt-setup-row" aria-expanded={totpOpen} onClick={() => setTotpOpen(!totpOpen)}>
                    <Plus />
                    Set up TOTP
                  </button>
                </div>
              )}
            </div>
            {totpOpen &&
              (entry ? (
                <TotpSetupPanel
                  title={entry.totp ? "TOTP settings" : "Set up TOTP"}
                  load={totpReady ? () => api.revealTotpSetup(entry.vault, entry.id) : undefined}
                  note="Saved to the Entry at once, encrypted like its password."
                  onSave={async (setup) => {
                    await applyNow(() => api.setTotp(entry.vault, entry.id, setup));
                    setTotpOpen(false);
                    toast(entry.totp ? "TOTP settings saved" : `TOTP set up for ${entry.title}`, { icon: "check" });
                  }}
                  onRemove={
                    entry.totp
                      ? async () => {
                          await applyNow(() => api.removeTotp(entry.vault, entry.id));
                          setTotpOpen(false);
                          toast(<>TOTP removed<span className="subtle"> · the earlier version keeps it in History</span></>, { icon: Timer });
                        }
                      : undefined
                  }
                  onClose={() => setTotpOpen(false)}
                />
              ) : (
                <TotpSetupPanel
                  title={stagedTotp ? "TOTP settings" : "Set up TOTP"}
                  initial={stagedTotp}
                  onSave={async (setup) => {
                    await api.previewTotp(setup);
                    setStagedTotp(setup);
                    setTotpOpen(false);
                  }}
                  onRemove={
                    stagedTotp
                      ? async () => {
                          setStagedTotp(null);
                          setTotpOpen(false);
                        }
                      : undefined
                  }
                  onClose={() => setTotpOpen(false)}
                />
              ))}
            <div className="ee-kv top">
              <span className="k pt-1.5">Notes</span>
              <textarea className="gi" aria-label="Notes" rows={2} value={draft.notes} placeholder="Add notes…" onChange={(event) => update({ notes: event.target.value })} />
            </div>
          </div>

          {draft.facets.map((facet) => (
            <FacetEditor
              key={`${facet.plugin}/${facet.facet}`}
              facet={facet}
              checks={checks}
              shown={shownFields}
              onShow={(row, show) => showRow(row.key, show)}
              onRow={(key, change) => updateFacets((facets) => withChange(facets, key, change))}
              onFormatted={(key, typed, formatted) => updateFacets((facets) => withFormatted(facets, key, typed, formatted))}
              onReveal={revealOwned}
              onRemove={() => update({ facets: draft.facets.filter((candidate) => !sameFacet(candidate, facet)) })}
            />
          ))}

          <div className="ee-sec">
            <div className="ee-sec-head">
              <h3>Custom fields</h3>
              <span className="n">{draft.fields.length}</span>
              <div className="ml-auto flex items-center gap-1">
                {addableFacets(templates, draft.facets).map((template) => (
                  <Button
                    key={`${template.plugin}/${template.facet}`}
                    variant="ghost"
                    size="sm"
                    title={template.editor!.add}
                    aria-label={template.editor!.add}
                    onClick={() => {
                      const added = addFacet(template, draft.fields, rowKey);
                      update({ fields: added.fields, facets: [...draft.facets, added.facet] });
                      // New values show as they are typed, as a new custom field's do.
                      setShownFields((current) => {
                        const next = new Set(current);
                        for (const row of added.facet.rows) if (row.original === null) next.add(row.key);
                        return next;
                      });
                      const first = added.facet.rows[0]?.key;
                      requestAnimationFrame(() => doc.current?.querySelector<HTMLInputElement>(`[data-owned-field="${first}"]`)?.focus());
                    }}
                  >
                    <Plus /> {template.title}
                  </Button>
                ))}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    const key = rowKey();
                    update({ fields: [...draft.fields, { key, name: "", value: "", protected: false, original: null, savedValue: null }] });
                    setShownFields((current) => new Set(current).add(key));
                    requestAnimationFrame(() => doc.current?.querySelector<HTMLInputElement>(`[data-field-name="${key}"]`)?.focus());
                  }}
                >
                  <Plus /> Field
                </Button>
              </div>
            </div>
            <div className="vstack gap-1.5">
              {draft.fields.length === 0 ? (
                <div className="ee-empty">No custom fields.</div>
              ) : (
                draft.fields.map((row) => {
                  const shown = !row.protected || shownFields.has(row.key);
                  const index = savedIndex(entry, row.original);
                  const kept = index !== null && (row.value === null || row.value === row.savedValue) && row.name === row.original;
                  return (
                    <div key={row.key} className="ee-cf">
                      <input
                        className={cn("gi", found?.fields.has(row.key) && "invalid")}
                        data-field-name={row.key}
                        aria-label="Field name"
                        value={row.name}
                        placeholder="Name"
                        spellCheck={false}
                        onChange={(event) => updateRow(row.key, { name: event.target.value })}
                      />
                      <div className="gi-group">
                        <input
                          aria-label={`${row.name || "Field"} value`}
                          className={cn(row.protected && "mono", row.value === null && "kept")}
                          type={shown ? "text" : "password"}
                          value={row.value ?? ""}
                          placeholder={row.value === null ? "••••••••••" : "Value"}
                          spellCheck={false}
                          autoComplete="off"
                          onFocus={() => void revealField(row)}
                          onChange={(event) => updateRow(row.key, { value: event.target.value })}
                        />
                        {row.protected && (
                          <IconButton
                            size="sm"
                            label={shown ? "Hide" : "Show"}
                            onClick={async () => {
                              if (!shown) await revealField(row);
                              setShownFields((current) => {
                                const next = new Set(current);
                                if (shown) next.delete(row.key);
                                else next.add(row.key);
                                return next;
                              });
                            }}
                          >
                            {shown ? <EyeOff /> : <Eye />}
                          </IconButton>
                        )}
                        <IconButton
                          size="sm"
                          label={kept ? "Copy" : "Copy the saved value: save first"}
                          disabled={!kept || !entry}
                          onClick={() => entry && row.original !== null && void copyField(entry.vault, entry.id, { kind: "customField", name: row.original }, row.name)}
                        >
                          <Copy />
                        </IconButton>
                      </div>
                      <label className="ee-prot" title="Protected values are masked and kept encrypted in memory">
                        <Switch
                          label="Protected"
                          checked={row.protected}
                          onCheckedChange={(on) => {
                            updateRow(row.key, { protected: on });
                            if (!on) void revealField(row);
                          }}
                        />
                        Protected
                      </label>
                      <IconButton
                        size="sm"
                        label="Remove"
                        onClick={() => update({ fields: draft.fields.filter((candidate) => candidate.key !== row.key) })}
                      >
                        <X />
                      </IconButton>
                    </div>
                  );
                })
              )}
            </div>
          </div>

          <Attachments entry={entry} applyNow={applyNow} />

          <div className="ee-hr" />
          <div className="ee-foot">
            {home ? <GroupIcon icon={home.icon} /> : <Folder />}
            <span className="truncate">{[vaultName, ...place].join(" / ")}</span>
            {entry?.created != null && (
              <>
                <span>·</span>
                <span>Created {formatDate(entry.created)}</span>
              </>
            )}
            {entry?.modified != null && (
              <>
                <span>·</span>
                <span>Modified {formatDate(entry.modified)}</span>
              </>
            )}
            <span className="spacer" />
            {entry && entry.historyCount > 0 && (
              <Button variant="ghost" size="sm" onClick={() => leave(() => actions.showHistory())}>
                <History /> {entry.historyCount} version{entry.historyCount === 1 ? "" : "s"}
              </Button>
            )}
          </div>
          {!entry && (
            <p className="hint mt-3">
              New Entries go into {[vaultName, ...place].join(" / ")}. <Kbd combo="Ctrl S" /> creates it.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

/** How strong the password being edited is: the saved one's (`password` null) as the backend
 * estimated it, without revealing it, or the one typed, estimated as it is typed. */
function usePasswordStrength(entry: EntryDetail | null, password: string | null): StrengthView | null {
  const [typed, setTyped] = useState<StrengthView | null>(null);
  useEffect(() => {
    if (!password) return;
    let current = true;
    const timer = setTimeout(() => {
      api.estimateStrength(password).then(
        (strength) => current && setTyped(strength),
        () => {},
      );
    }, 100);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [password]);
  if (password === null) return entry?.strength ?? null;
  return password ? typed : null;
}
