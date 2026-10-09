// The popup: says whether keefast can fill sign-in forms, pairs the browser, lists the page's
// Entries to fill, with their TOTP codes on a 2FA step, and fills new passwords from keefast's
// generator. Its settings turn keefast's icons in sign-in fields on and off. It never asks for
// a master password: keefast is unlocked in keefast itself (ADR 0001).
import { useCallback, useEffect, useState } from "react";
import { browser, type Browser } from "wxt/browser";
import { RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import type { EntrySummary } from "@/companion";
import { explain } from "@/explain";
import { type PageFields, isTwoFactorStep } from "@/forms";
import type { FailureReason } from "@/keefast";
import type { Answer, Answers, Request } from "@/messages";
import { ALL_SITES, addSiteOnPick, hiddenSites } from "@/settings";

async function ask<R extends Request>(request: R): Promise<Answer<Answers[R["type"]]>> {
  return browser.runtime.sendMessage(request);
}

type View =
  | { kind: "message"; title: string; detail?: string; retry?: boolean }
  | { kind: "pair" }
  | { kind: "entries"; tab: number; entries: EntrySummary[]; fields: PageFields };

function failure(reason: FailureReason, retry = false): View {
  const [title, detail] = explain(reason);
  return { kind: "message", title, detail, retry };
}

async function activeTab(): Promise<Browser.tabs.Tab | undefined> {
  const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
  return tab;
}

export function Popup() {
  const [view, setView] = useState<View>({ kind: "message", title: "Connecting to keefast…" });

  const listEntries = useCallback(async () => {
    const tab = (await activeTab())?.id;
    if (tab === undefined)
      return setView({ kind: "message", title: "keefast can't fill this page." });
    setView({
      kind: "message",
      title: "Asking keefast for this site's Entries…",
      detail: "If keefast asks, allow this site there.",
    });
    const answer = await ask({ type: "entries", tab });
    if (!answer.ok) return setView(failure(answer.reason));
    if (answer.value === null) {
      return setView({ kind: "message", title: "keefast can't fill this page." });
    }
    setView({ kind: "entries", tab, ...answer.value });
  }, []);

  const start = useCallback(async () => {
    setView({ kind: "message", title: "Connecting to keefast…" });
    const status = await ask({ type: "status" });
    if (!status.ok) return setView(failure(status.reason, true));
    switch (status.value) {
      case "not-running":
      case "locked":
        return setView(failure(status.value, true));
      case "unpaired":
        return setView({ kind: "pair" });
      case "paired":
        return listEntries();
    }
  }, [listEntries]);

  useEffect(() => void start(), [start]);

  const pair = async () => {
    setView({
      kind: "message",
      title: "Approve the Pairing in keefast.",
      detail: "keefast asks you to name this browser.",
    });
    const paired = await ask({ type: "pair" });
    if (!paired.ok) return setView(failure(paired.reason, true));
    await listEntries();
  };

  return (
    <div className="w-[340px] text-sm">
      <header className="flex items-center gap-2 border-b px-3 py-2.5 font-semibold">
        <img src="/icons/icon-32.png" alt="" width={20} height={20} />
        keefast
      </header>
      <main className="space-y-3 p-3" aria-live="polite">
        {view.kind === "message" && (
          <>
            <Message title={view.title} detail={view.detail} />
            {view.retry && <Button onClick={() => void start()}>Try again</Button>}
          </>
        )}
        {view.kind === "pair" && (
          <>
            <Message
              title="Connect this browser to keefast."
              detail="You approve the Pairing in keefast, which then fills sign-in forms when you ask."
            />
            <Button onClick={() => void pair()}>Connect</Button>
          </>
        )}
        {view.kind === "entries" && (
          <EntryList
            tab={view.tab}
            entries={view.entries}
            fields={view.fields}
            onFailed={(failed) => setView(failed)}
          />
        )}
      </main>
      <Separator />
      <Settings />
    </div>
  );
}

function Message({ title, detail }: { title: string; detail?: string }) {
  return (
    <div className="space-y-0.5">
      <p className="font-medium">{title}</p>
      {detail && <p className="text-muted-foreground">{detail}</p>}
    </div>
  );
}

function EntryList({
  tab,
  entries,
  fields,
  onFailed,
}: {
  tab: number;
  entries: EntrySummary[];
  fields: PageFields;
  onFailed: (view: View) => void;
}) {
  const twoFactor = isTwoFactorStep(fields);

  const fill = async (entry: EntrySummary) => {
    const filled = await ask({ type: "fill", tab, uuid: entry.uuid });
    if (filled.ok && filled.value) return window.close();
    if (!filled.ok) return onFailed(failure(filled.reason));
    const why = fields.login
      ? "This page has no login form, or it changed."
      : "This page changed, or keefast no longer gives this Entry's code here.";
    onFailed({
      kind: "message",
      title: "Nothing was filled.",
      detail: `${why} Open the popup again.`,
    });
  };

  return (
    <>
      {entries.length === 0 ? (
        twoFactor ? (
          <Message
            title="No Entry for this site has TOTP."
            detail="Set up TOTP on its Entry in keefast."
          />
        ) : (
          <Message
            title="No Entries for this site."
            detail="Sign in or sign up, and keefast offers to save it."
          />
        )
      ) : (
        <ul className="-mx-1 space-y-0.5">
          {entries.map((entry, index) => (
            <li key={entry.uuid}>
              <button
                type="button"
                // A sign-up has more use for the generator.
                autoFocus={index === 0 && !fields.newPassword}
                onClick={() => void fill(entry)}
                title={
                  entry.hasTotp && fields.totp
                    ? fields.login
                      ? "Fills the login and its TOTP code"
                      : "Fills its TOTP code"
                    : undefined
                }
                className="flex w-full flex-col items-start rounded-md px-2 py-1.5 text-left outline-none hover:bg-accent focus-visible:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <span className="flex items-center gap-2 font-medium">
                  {entry.name || entry.username}
                  {entry.hasTotp && fields.totp && (
                    <Badge variant="secondary" className="px-1.5 py-0 text-[10px]">
                      TOTP
                    </Badge>
                  )}
                </span>
                {entry.username && <span className="text-muted-foreground">{entry.username}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
      {/* A 2FA step has no use for a new password; a sign-up has no use for much else. */}
      {!twoFactor && <Generator tab={tab} fields={fields} />}
    </>
  );
}

/**
 * The generator: makes a password with keefast's generator settings, shows it, and fills it
 * into the page's new password fields, when the page has them.
 */
function Generator({ tab, fields }: { tab: number; fields: PageFields }) {
  const [password, setPassword] = useState<string>();
  const [notice, setNotice] = useState<{ title: string; detail?: string }>();

  const generate = async () => {
    setNotice(undefined);
    const generated = await ask({ type: "generate", tab });
    if (generated.ok && generated.value !== null) return setPassword(generated.value);
    setPassword(undefined);
    if (!generated.ok && generated.reason === "denied") {
      setNotice({
        title: "keefast's generator settings can't make a password.",
        detail: "Change them in keefast.",
      });
    } else if (!generated.ok) {
      const [title, detail] = explain(generated.reason);
      setNotice({ title, detail });
    } else {
      setNotice({ title: "keefast can't fill this page." });
    }
  };

  const fill = async () => {
    const filled = await ask({ type: "fill-generated", tab });
    if (filled.ok && filled.value) return window.close();
    setPassword(undefined);
    setNotice({
      title: "Nothing was filled.",
      detail: "This page has no new password field, or it changed. Open the popup again.",
    });
  };

  return (
    <section className="space-y-2 border-t pt-3">
      {password === undefined ? (
        <Button
          autoFocus={fields.newPassword}
          variant={fields.newPassword ? "default" : "secondary"}
          onClick={() => void generate()}
        >
          Generate password
        </Button>
      ) : (
        <>
          <Input
            readOnly
            value={password}
            aria-label="Generated password"
            spellCheck={false}
            className="font-mono"
          />
          <div className="flex gap-2">
            {fields.newPassword && (
              <Button autoFocus onClick={() => void fill()}>
                Fill
              </Button>
            )}
            <Button variant="secondary" onClick={() => void generate()}>
              <RefreshCw />
              Another
            </Button>
          </div>
          {!fields.newPassword && (
            <p className="text-muted-foreground">This page has no new password field to fill.</p>
          )}
        </>
      )}
      {notice && <Message {...notice} />}
    </section>
  );
}

/** The extension's settings, kept on this browser only. */
function Settings() {
  const [fieldIcons, setFieldIcons] = useState(false);
  const [addSite, setAddSite] = useState(false);
  const [hidden, setHidden] = useState<{ tab: number; host: string }>();

  useEffect(() => {
    void (async () => {
      setFieldIcons(await browser.permissions.contains(ALL_SITES));
      setAddSite(await addSiteOnPick.getValue());
      const tab = await activeTab();
      if (tab?.id === undefined || !tab.url) return;
      const url = new URL(tab.url);
      if ((await hiddenSites.getValue()).includes(url.origin)) {
        setHidden({ tab: tab.id, host: url.host });
      }
    })();
  }, []);

  const toggleFieldIcons = async (on: boolean) => {
    if (on) {
      // Asked of the user here, while the popup has their click.
      const granted = await browser.permissions.request(ALL_SITES);
      setFieldIcons(granted);
      const tab = (await activeTab())?.id;
      if (granted && tab !== undefined) await ask({ type: "show-on-site", tab });
    } else {
      await browser.permissions.remove(ALL_SITES);
      setFieldIcons(false);
    }
  };

  const showAgain = async () => {
    if (!hidden) return;
    await ask({ type: "show-on-site", tab: hidden.tab });
    setHidden(undefined);
  };

  return (
    <section className="space-y-3 p-3">
      <Setting
        id="field-icons"
        label="Show keefast in sign-in fields"
        detail="Lets keefast see every page you open, to find their sign-in fields."
        checked={fieldIcons}
        onChange={(on) => void toggleFieldIcons(on)}
      />
      {fieldIcons && hidden && (
        <div className="flex items-center justify-between gap-2 text-muted-foreground">
          <span className="truncate">Hidden on {hidden.host}</span>
          <Button size="sm" variant="secondary" onClick={() => void showAgain()}>
            Show again
          </Button>
        </div>
      )}
      <Setting
        id="add-site"
        label="Add the site to Entries I fill from search"
        detail="keefast then offers the Entry on the site from the start."
        checked={addSite}
        onChange={(on) => {
          setAddSite(on);
          void addSiteOnPick.setValue(on);
        }}
      />
    </section>
  );
}

function Setting({
  id,
  label,
  detail,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  detail: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="space-y-0.5">
        <Label htmlFor={id}>{label}</Label>
        <p className="text-xs text-muted-foreground">{detail}</p>
      </div>
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
    </div>
  );
}
