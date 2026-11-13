// Vault browser C's inspector: the Entry's hero, the username / password / TOTP tiles, and its
// Overview, Fields, Files, History and Access tabs.

import { type ReactNode, useEffect, useRef, useState } from "react";
import {
  ArrowUpRight, Copy, Ellipsis, Eye, EyeOff, FileKey, FileText, Folder, Globe, KeyRound, Pencil, Plus,
  Settings2, Timer, Trash2, User,
} from "lucide-react";

import { api, errorMessage } from "@/api";
import type { CopyTarget, EntryDetail as Entry } from "@/bindings";
import { useTotpCode } from "@/api/queries";
import {
  Badge, Button, colorVar, Kbd, Menu, MenuContent, MenuItem, MenuSeparator, MenuTrigger, Tabs, TabsContent, TabsList,
  TabsTrigger, toast,
} from "@/kit";
import { describeExpiry, formatDate, formatSize, hostOf } from "@/lib/format";
import { SHORTCUTS } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { HistoryTab } from "@/screens/entry-history/HistoryTab";
import { useShowHistoryCommand } from "@/screens/entry-history/open";
import { FacetCommands } from "@/screens/plugins/commands";
import { FacetSection } from "@/screens/plugins/FacetSection";
import { TotpSetupPanel } from "@/screens/totp/TotpSetup";
import { useAppCommand } from "@/shell/commands";
import { useKeysActive } from "@/shell/hooks";
import { COLOR_NAMES } from "@/shell/Sidebar";
import { useShell } from "@/shell/state";

import { copyField, deleteEntry, openUrl } from "./actions";
import { EntryAvatar } from "./EntryList";
import { type CopyHandler, FieldRow } from "./FieldRow";

const MASK = "••••••••••••";
/** How long a copied field shows "Copied". */
const FLASH_MS = 1200;
const UNREADABLE_TOTP = "Can't be read · Set up again";

type Tab = "overview" | "fields" | "files" | "history" | "access";

export function EntryDetail({ entry }: { entry: Entry }) {
  const { actions } = useShell();
  const keysActive = useKeysActive();
  const [tab, setTab] = useState<Tab>("overview");
  const [password, setPassword] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [totpSetup, setTotpSetup] = useState(false);
  const flash = useRef<ReturnType<typeof setTimeout>>(undefined);
  const totp = useTotpCode(entry.vault, entry.id, entry.totp?.status === "ready");

  // A revealed password goes when the window is left.
  useEffect(() => {
    const hide = () => setPassword(null);
    window.addEventListener("blur", hide);
    return () => {
      window.removeEventListener("blur", hide);
      clearTimeout(flash.current);
    };
  }, []);

  const copy = async (field: CopyTarget, what: string, key: string) => {
    if (await copyField(entry.vault, entry.id, field, what)) {
      setCopied(key);
      clearTimeout(flash.current);
      flash.current = setTimeout(() => setCopied(null), FLASH_MS);
    }
  };
  const togglePassword = async () => {
    try {
      setPassword(password === null ? await api.revealPassword(entry.vault, entry.id) : null);
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };

  const hasTotp = entry.totp?.status === "ready";
  useAppCommand(
    { id: "copyUsername", title: "Copy username", section: "Entry", icon: User, shortcut: SHORTCUTS.copyUsername, enabled: !!entry.username, run: () => void copy({ kind: "username" }, "Username", "username") },
    keysActive,
  );
  useAppCommand(
    {
      id: "copyPassword",
      title: "Copy password",
      section: "Entry",
      icon: KeyRound,
      shortcut: SHORTCUTS.copyPassword,
      enabled: entry.hasPassword,
      // Ctrl C copies the text the user selected, when there is some.
      when: () => !window.getSelection()?.toString(),
      run: () => void copy({ kind: "password" }, "Password", "password"),
    },
    keysActive,
  );
  useAppCommand(
    { id: "copyTotp", title: "Copy TOTP code", section: "Entry", icon: Timer, shortcut: SHORTCUTS.copyTotp, enabled: hasTotp, run: () => void copy({ kind: "totp" }, "TOTP code", "totp") },
    keysActive,
  );
  useAppCommand(
    {
      id: "editEntry",
      title: "Edit Entry",
      section: "Entry",
      icon: Pencil,
      shortcut: SHORTCUTS.edit,
      // Enter on a button presses it rather than editing.
      when: (event) => !(event.target instanceof Element && event.target.closest("button, a, [role=button], [role=tab], [role=menuitem]")),
      run: actions.editEntry,
    },
    keysActive,
  );
  useShowHistoryCommand(actions, keysActive);
  useAppCommand({ id: "openUrl", title: "Open URL", section: "Entry", icon: Globe, enabled: !!entry.url, run: () => openUrl(entry.vault, entry.id) }, keysActive);

  const location = entry.groupPath.length > 1 ? `${entry.vaultName} / ${entry.groupPath[entry.groupPath.length - 1]}` : entry.vaultName;

  return (
    <div className="vb-detail-scroll">
      <FacetCommands entry={entry} active={keysActive} onCopy={copy} />
      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
        <div className="C-insp-top">
          <div className="C-hero">
            <EntryAvatar entry={entry} size={48} />
            <div className="min-w-0 flex-1 pt-0.5">
              <h1 className="vb-title truncate selectable">{entry.title}</h1>
              <div className="sub">
                {entry.url && (
                  <>
                    <button type="button" className="vb-link truncate text-fg-1" onClick={() => openUrl(entry.vault, entry.id)}>
                      {hostOf(entry.url)}
                      <ArrowUpRight />
                    </button>
                    <span>·</span>
                  </>
                )}
                <span className="truncate">{location}</span>
                {entry.expired && <Badge tone="danger" className="ml-0.5">Expired</Badge>}
              </div>
            </div>
            <Button variant="secondary" size="sm" shortcut="E" onClick={actions.editEntry}>
              <Pencil /> Edit
            </Button>
            <EntryMenu entry={entry} onDelete={() => void deleteEntry(entry.vault, entry.id, actions)} />
          </div>
          <div className="C-quick">
            <QuickTile
              label="Username"
              icon={<User className="ico-sm" />}
              shortcut="Ctrl B"
              copied={copied === "username"}
              onCopy={entry.username ? () => void copy({ kind: "username" }, "Username", "username") : undefined}
            >
              {entry.username ? <span className="truncate selectable">{entry.username}</span> : <span className="subtle">None</span>}
            </QuickTile>
            <QuickTile
              label="Password"
              icon={<KeyRound className="ico-sm" />}
              shortcut="Ctrl C"
              copied={copied === "password"}
              onCopy={entry.hasPassword ? () => void copy({ kind: "password" }, "Password", "password") : undefined}
              extra={
                entry.hasPassword && (
                  <button
                    type="button"
                    className="btn ghost sm icon eye"
                    aria-label={password === null ? "Show password" : "Hide password"}
                    title={password === null ? "Show" : "Hide"}
                    onClick={() => void togglePassword()}
                  >
                    {password === null ? <Eye /> : <EyeOff />}
                  </button>
                )
              }
            >
              {!entry.hasPassword ? (
                <span className="subtle">None</span>
              ) : password === null ? (
                <span className="vb-pw masked">{MASK}</span>
              ) : (
                <span className="mono truncate pr-7 selectable">{password}</span>
              )}
            </QuickTile>
            {entry.totp?.status === "ready" ? (
              <QuickTile
                label="TOTP"
                icon={<Timer className="ico-sm" />}
                shortcut="Ctrl T"
                copied={copied === "totp"}
                onCopy={() => void copy({ kind: "totp" }, "TOTP code", "totp")}
                open={totpSetup}
                extra={
                  <button
                    type="button"
                    className={cn("btn ghost sm icon eye tt-tile-btn", totpSetup && "is-on")}
                    aria-label="TOTP settings"
                    aria-expanded={totpSetup}
                    title="TOTP…"
                    onClick={() => setTotpSetup(!totpSetup)}
                  >
                    <Settings2 />
                  </button>
                }
              >
                {totp.data ? <TotpCode code={totp.data.code} remaining={totp.data.remaining} period={totp.data.period} /> : <span className="subtle">…</span>}
              </QuickTile>
            ) : (
              <button
                type="button"
                className={cn("C-qt is-setup is-action", totpSetup && "is-open")}
                aria-expanded={totpSetup}
                title={entry.totp?.status === "unreadable" ? `${UNREADABLE_TOTP}\n${entry.totp.reason}` : undefined}
                onClick={() => setTotpSetup(!totpSetup)}
              >
                <span className="k">
                  <Timer className="ico-sm" /> TOTP
                </span>
                <span className="v subtle">
                  {entry.totp?.status === "unreadable" ? (
                    // In a span of its own: the tile's ellipsis doesn't show on bare text in a flex box.
                    <span className="truncate">{UNREADABLE_TOTP}</span>
                  ) : (
                    <>
                      <Plus className="ico-sm" /> Set up TOTP
                    </>
                  )}
                </span>
              </button>
            )}
          </div>
          {totpSetup && (
            <TotpSetupPanel
              title={entry.totp ? "TOTP settings" : "Set up TOTP"}
              load={entry.totp?.status === "ready" ? () => api.revealTotpSetup(entry.vault, entry.id) : undefined}
              onSave={async (setup) => {
                await api.setTotp(entry.vault, entry.id, setup);
                setTotpSetup(false);
                toast(entry.totp ? "TOTP settings saved" : `TOTP set up for ${entry.title}`, { icon: "check" });
              }}
              onRemove={
                entry.totp
                  ? async () => {
                      await api.removeTotp(entry.vault, entry.id);
                      setTotpSetup(false);
                      toast(<>TOTP removed<span className="subtle"> · the earlier version keeps it in History</span></>, { icon: Timer });
                    }
                  : undefined
              }
              onClose={() => setTotpSetup(false)}
            />
          )}
          <TabsList className="C-tabs mt-5">
            <TabsTrigger value="overview">Overview</TabsTrigger>
            <TabsTrigger value="fields" count={shownCustomFields(entry).length}>Fields</TabsTrigger>
            <TabsTrigger value="files" count={entry.attachments.length}>Files</TabsTrigger>
            <TabsTrigger value="history" count={entry.historyCount}>History</TabsTrigger>
            <TabsTrigger value="access" count={entry.rememberedAccess.length}>Access</TabsTrigger>
          </TabsList>
        </div>
        <div className="C-pane">
          <TabsContent value="overview">
            <Overview entry={entry} copied={copied} onCopy={copy} />
          </TabsContent>
          <TabsContent value="fields">
            <Fields entry={entry} copied={copied} onCopy={copy} />
          </TabsContent>
          <TabsContent value="files">
            <Files entry={entry} />
          </TabsContent>
          <TabsContent value="history">
            <HistoryTab entry={entry} />
          </TabsContent>
          <TabsContent value="access">
            <Access entry={entry} />
          </TabsContent>
        </div>
      </Tabs>
    </div>
  );
}

function EntryMenu({ entry, onDelete }: { entry: Entry; onDelete: () => void }) {
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="btn ghost sm icon" aria-label="More">
          <Ellipsis />
        </button>
      </MenuTrigger>
      <MenuContent align="end">
        <MenuItem icon={<Globe />} disabled={!entry.url} onSelect={() => openUrl(entry.vault, entry.id)}>
          Open URL
        </MenuItem>
        <MenuItem icon={<Copy />} disabled={!entry.url} onSelect={() => void copyField(entry.vault, entry.id, { kind: "url" }, "URL")}>
          Copy URL
        </MenuItem>
        <MenuSeparator />
        <MenuItem icon={<Trash2 />} shortcut="Delete" danger onSelect={onDelete}>
          {entry.deletesForGood ? "Delete for good…" : "Delete"}
        </MenuItem>
      </MenuContent>
    </Menu>
  );
}

function QuickTile({
  label,
  icon,
  shortcut,
  copied,
  onCopy,
  extra,
  open,
  children,
}: {
  label: string;
  icon: ReactNode;
  shortcut: string;
  copied: boolean;
  onCopy?: () => void;
  extra?: ReactNode;
  /** Its setup is expanded under the tiles. */
  open?: boolean;
  children: ReactNode;
}) {
  // A div with a button role: the eye inside is a button of its own.
  return (
    <div
      role="button"
      tabIndex={onCopy ? 0 : -1}
      aria-label={`Copy ${label.toLowerCase()}`}
      aria-disabled={!onCopy}
      className={cn("C-qt", copied && "is-copied", open && "is-open")}
      onClick={(event) => {
        if ((event.target as Element).closest(".eye")) return;
        onCopy?.();
      }}
      onKeyDown={(event) => {
        if ((event.key === "Enter" || event.key === " ") && event.target === event.currentTarget) {
          event.preventDefault();
          onCopy?.();
        }
      }}
    >
      <span className="k">
        {icon} {label} {onCopy && <Kbd combo={shortcut} />}
      </span>
      <span className="v">{children}</span>
      {extra}
    </div>
  );
}

function TotpCode({ code, remaining, period }: { code: string; remaining: number; period: number }) {
  const [left, setLeft] = useState(remaining);
  useEffect(() => {
    setLeft(remaining);
    const timer = setInterval(() => setLeft((value) => Math.max(0, value - 1)), 1000);
    return () => clearInterval(timer);
  }, [code, remaining]);
  const half = Math.ceil(code.length / 2);
  return (
    <span className={cn("vb-totp", left <= 5 && "is-low")}>
      <span className="ring" style={{ "--p": left / period } as React.CSSProperties} />
      <span className="code">
        {code.slice(0, half)} {code.slice(half)}
      </span>
      <span className="left">{left}s</span>
    </span>
  );
}

function Overview({ entry, copied, onCopy }: { entry: Entry; copied: string | null; onCopy: CopyHandler }) {
  const expiry = describeExpiry(entry.expires, entry.expired);
  return (
    <>
      {entry.facets.map((facet) => (
        <FacetSection key={`${facet.plugin}:${facet.facet}`} entry={entry} facet={facet} copied={copied} onCopy={onCopy} />
      ))}
      <div className="vb-fields">
        <FieldRow label="URL" copied={copied === "url"} onCopy={entry.url ? () => void onCopy({ kind: "url" }, "URL", "url") : undefined}>
          {entry.url ? (
            <button type="button" className="vb-link truncate" onClick={() => openUrl(entry.vault, entry.id)}>
              <span className="truncate">{entry.url}</span>
              <ArrowUpRight />
            </button>
          ) : (
            <span className="subtle">None</span>
          )}
        </FieldRow>
        {entry.additionalUrls.map((url, index) => (
          <FieldRow key={index} label="Also for">
            <span className="val truncate selectable">{url}</span>
          </FieldRow>
        ))}
      </div>
      <div className="vb-sect">
        <div className="vb-sect-head">Notes</div>
        {entry.notes ? <div className="vb-notes">{entry.notes}</div> : <div className="vb-notes empty">No notes.</div>}
      </div>
      <div className="vb-sect">
        <div className="vb-sect-head">Details</div>
        <div className="vb-fields px-3.5 py-1">
          <Prop label="Group">
            <span className="vb-chip">
              <Folder />
              {entry.groupPath.length > 1 ? entry.groupPath[entry.groupPath.length - 1] : entry.vaultName}
            </span>
          </Prop>
          <Prop label="Tags">
            {entry.tags.length ? (
              entry.tags.map((tag) => (
                <span key={tag} className="tag">
                  <span className="dot" />
                  {tag}
                </span>
              ))
            ) : (
              <span className="subtle">None</span>
            )}
          </Prop>
          <Prop label="Colour">
            {entry.color ? (
              <span className="hstack gap-1.5">
                <span className="dot" style={{ background: colorVar(entry.color) }} />
                {COLOR_NAMES[entry.color]}
              </span>
            ) : (
              <span className="subtle">None</span>
            )}
          </Prop>
          <Prop label="Expires">
            {expiry.kind === "never" && <span className="subtle">Never</span>}
            {expiry.kind === "expired" && (
              <>
                <Badge tone="danger">Expired</Badge>
                <span className="subtle">{expiry.date}</span>
              </>
            )}
            {expiry.kind === "soon" && (
              <>
                <Badge tone="warn">{expiry.days === 0 ? "Today" : `In ${expiry.days} day${expiry.days === 1 ? "" : "s"}`}</Badge>
                <span className="subtle">{expiry.date}</span>
              </>
            )}
            {expiry.kind === "later" && expiry.date}
          </Prop>
          <Prop label="Modified">{entry.modified !== null ? formatDate(entry.modified) : <span className="subtle">Unknown</span>}</Prop>
          <Prop label="Created">{entry.created !== null ? formatDate(entry.created) : <span className="subtle">Unknown</span>}</Prop>
        </div>
      </div>
    </>
  );
}

function Prop({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="vb-prop">
      <span className="k">{label}</span>
      <span className="v">{children}</span>
    </div>
  );
}

function SectionHead({ title, count, children }: { title: string; count: number; children?: ReactNode }) {
  return (
    <div className="vb-sect-head">
      {title} <span className="count">{count}</span>
      {children}
    </div>
  );
}

/** The custom fields the Fields tab lists, by their index in the Entry: those TOTP settings are
 * kept in are the TOTP tile's while the Entry has TOTP. */
function shownCustomFields(entry: Entry) {
  return entry.customFields.flatMap((field, index) => (entry.totp && field.totpStorage ? [] : [{ field, index }]));
}

function Fields({ entry, copied, onCopy }: { entry: Entry; copied: string | null; onCopy: CopyHandler }) {
  // By field name: a Merge that adds or reorders fields doesn't reveal another one.
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  useEffect(() => {
    const hide = () => setRevealed({});
    window.addEventListener("blur", hide);
    return () => window.removeEventListener("blur", hide);
  }, []);
  const toggle = async (name: string) => {
    if (name in revealed) {
      const next = { ...revealed };
      delete next[name];
      return setRevealed(next);
    }
    try {
      const value = await api.revealCustomField(entry.vault, entry.id, name);
      setRevealed((current) => ({ ...current, [name]: value }));
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  const fields = shownCustomFields(entry);
  return (
    <>
      <SectionHead title="Custom fields" count={fields.length} />
      {fields.length === 0 ? (
        <div className="vb-notes empty">No custom fields.</div>
      ) : (
        <div className="vb-fields">
          {fields.map(({ field, index }) => {
            const value = field.value ?? revealed[field.name];
            const key = `field:${index}`;
            return (
              <FieldRow
                key={index}
                label={field.name}
                copied={copied === key}
                onCopy={field.isEmpty ? undefined : () => void onCopy({ kind: "customField", name: field.name }, field.name, key)}
                actions={
                  field.protected &&
                  !field.isEmpty && (
                    <button type="button" className="btn ghost sm icon" aria-label={field.name in revealed ? "Hide" : "Show"} title={field.name in revealed ? "Hide" : "Show"} onClick={() => void toggle(field.name)}>
                      {field.name in revealed ? <EyeOff /> : <Eye />}
                    </button>
                  )
                }
              >
                {field.isEmpty ? (
                  <span className="subtle">Empty</span>
                ) : value === undefined ? (
                  <span className="val vb-pw masked">{MASK.slice(0, 10)}</span>
                ) : (
                  <span className={cn("val selectable", field.protected && "vb-pw")}>{value}</span>
                )}
              </FieldRow>
            );
          })}
        </div>
      )}
    </>
  );
}

function Files({ entry }: { entry: Entry }) {
  const { actions } = useShell();
  const save = async (index: number) => {
    try {
      const path = await api.saveAttachment(entry.vault, entry.id, index);
      if (path) toast(`Saved to ${path}`);
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  return (
    <>
      <SectionHead title="Attachments" count={entry.attachments.length} />
      {entry.attachments.length === 0 ? (
        <div className="vb-notes empty">No attachments.</div>
      ) : (
        <div className="vb-fields">
          {entry.attachments.map((attachment) => (
            <div key={attachment.index} className="vb-file">
              <span className="vb-file-ico">{attachment.preview === "text" ? <FileText /> : <FileKey />}</span>
              <div className="vstack min-w-0 flex-1 gap-0">
                <span className="name truncate">{attachment.name}</span>
                <span className="size">{formatSize(attachment.size)}</span>
              </div>
              {attachment.preview !== "none" && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    actions.openOverlay({ kind: "attachment", vault: entry.vault, entry: entry.id, index: attachment.index, name: attachment.name })
                  }
                >
                  Preview
                </Button>
              )}
              <Button variant="ghost" size="sm" onClick={() => void save(attachment.index)}>
                Save…
              </Button>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

function Access({ entry }: { entry: Entry }) {
  const forget = async (host: string | null) => {
    try {
      await api.forgetAccess(entry.vault, entry.id, host);
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  const remembered = entry.rememberedAccess;
  return (
    <>
      <SectionHead title="Browser access" count={remembered.length}>
        {remembered.length > 0 && (
          <Button variant="ghost" size="sm" onClick={() => void forget(null)}>
            Forget all
          </Button>
        )}
      </SectionHead>
      {remembered.length === 0 ? (
        <div className="vb-notes empty">No site has asked for this Entry yet. Access decisions you remember show here.</div>
      ) : (
        <div className="vb-fields">
          {remembered.map((access) => (
            <div key={access.host} className="vb-acc">
              <Globe />
              <span className="host truncate">{access.host}</span>
              <Badge tone={access.allowed ? "ok" : "danger"}>{access.allowed ? "Allowed" : "Denied"}</Badge>
              <Button variant="ghost" size="sm" onClick={() => void forget(access.host)}>
                Forget
              </Button>
            </div>
          ))}
        </div>
      )}
      <p className="hint mt-3">An Access decision overrides the Vault's Default access policy for that site.</p>
    </>
  );
}
