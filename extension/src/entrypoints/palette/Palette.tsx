// The command palette, framed over the page by keefast's icon in a field or the shortcut. It
// lists the site's Entries, searches the paired Vaults for others as the user types, and fills
// the one picked; or, for a new password field, shows a password from keefast's generator to
// fill. It shows no passwords but a generated one (ADR 0007).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { browser } from "wxt/browser";
import { EyeOff, KeyRound, LoaderCircle, RefreshCw, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Input } from "@/components/ui/input";
import type { EntrySummary } from "@/companion";
import { explain } from "@/explain";
import type { FailureReason, FoundEntry } from "@/keefast";
import type {
  Answer,
  PaletteAnswers,
  PaletteMessage,
  PaletteRequest,
  PaletteStart,
} from "@/messages";

/** Proves to the service worker that keefast opened this palette. */
const token = location.hash.slice(1);

/** How long the user may pause typing before the Vaults are searched. */
const SEARCH_DELAY_MS = 150;

async function ask<R extends PaletteRequest>(
  request: R,
): Promise<Answer<PaletteAnswers[R["type"]]>> {
  const message: PaletteMessage = { type: "palette", token, request };
  const answer: Answer<PaletteAnswers[R["type"]]> | undefined = await browser.runtime
    .sendMessage(message)
    .catch(() => undefined);
  return answer ?? { ok: false, reason: "failed" };
}

const close = () => void ask({ type: "close" });

/** Whether an Entry listed for the site matches every word typed. */
function matches(entry: EntrySummary, query: string): boolean {
  const text = `${entry.name} ${entry.username} ${entry.group}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .every((word) => text.includes(word));
}

type Problem = { title: string; detail?: string };

function problem(reason: FailureReason): Problem {
  const [title, detail] = explain(reason);
  return { title, detail };
}

export function Palette() {
  const [start, setStart] = useState<PaletteStart>();
  const [failed, setFailed] = useState<Problem>();

  useEffect(() => {
    void ask({ type: "open" }).then((answer) => {
      if (answer.ok) setStart(answer.value);
      else setFailed(problem(answer.reason));
    });
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <div className="fixed inset-0 font-sans text-sm">
      <div
        className="absolute inset-0 bg-black/40 animate-in fade-in-0 duration-150"
        onClick={close}
        aria-hidden="true"
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-label="keefast"
        className="relative mx-auto mt-[12vh] w-[min(640px,calc(100vw-2rem))] overflow-hidden rounded-xl border bg-popover text-popover-foreground shadow-2xl animate-in fade-in-0 zoom-in-95 slide-in-from-top-2 duration-150 ease-out"
      >
        {failed ? (
          <Message {...failed} />
        ) : !start ? (
          <Waiting />
        ) : start.mode === "generate" ? (
          <Generator site={start.site} />
        ) : (
          <Entries start={start} />
        )}
      </div>
    </div>
  );
}

function Waiting() {
  return (
    <div className="flex items-start gap-3 p-4">
      <LoaderCircle className="mt-0.5 size-4 shrink-0 animate-spin text-muted-foreground" />
      <div>
        <p className="font-medium">Asking keefast for this site's Entries…</p>
        <p className="text-muted-foreground">If keefast asks, allow this site there.</p>
      </div>
    </div>
  );
}

function Message({ title, detail }: Problem) {
  return (
    <div className="space-y-1 p-4">
      <p className="font-medium">{title}</p>
      {detail && <p className="text-muted-foreground">{detail}</p>}
    </div>
  );
}

function Entries({ start }: { start: PaletteStart }) {
  const { site, entries } = start;
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<FoundEntry[]>([]);
  const [searching, setSearching] = useState(false);
  const [notice, setNotice] = useState<Problem>();
  const [filling, setFilling] = useState<string>();
  const searched = useRef(0);

  const listed = useMemo(
    () => (query.trim() ? entries.filter((entry) => matches(entry, query)) : entries),
    [entries, query],
  );
  const others = useMemo(() => {
    const forSite = new Set(entries.map(({ uuid }) => uuid));
    return found.filter(({ uuid }) => !forSite.has(uuid));
  }, [entries, found]);

  // The paired Vaults are searched as the user types: never for nothing typed.
  useEffect(() => {
    const search = ++searched.current;
    if (!query.trim()) {
      setFound([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    const timer = setTimeout(async () => {
      const answer = await ask({ type: "search", query });
      if (search !== searched.current) return;
      setSearching(false);
      if (answer.ok) setFound(answer.value);
      else setNotice(problem(answer.reason));
    }, SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [query]);

  const fill = useCallback(async (uuid: string, isFound: boolean) => {
    setFilling(uuid);
    setNotice(undefined);
    const answer = await ask({ type: "fill", uuid, found: isFound });
    setFilling(undefined);
    // Once filled, keefast closes the palette.
    if (!answer.ok) setNotice(problem(answer.reason));
    else if (!answer.value) {
      setNotice({
        title: "Nothing was filled.",
        detail: "This page changed, or has no field for this Entry any more.",
      });
    }
  }, []);

  const nothingForSite = entries.length === 0 && !query.trim();

  return (
    <Command shouldFilter={false} loop label="Fill with keefast">
      <CommandInput
        autoFocus
        value={query}
        onValueChange={setQuery}
        placeholder="Search keefast for an Entry…"
        className="h-12"
      />
      <CommandList className="max-h-[min(360px,60vh)]">
        {notice && (
          <div className="border-b px-4 py-3" role="status">
            <p className="font-medium">{notice.title}</p>
            {notice.detail && <p className="text-muted-foreground">{notice.detail}</p>}
          </div>
        )}
        {nothingForSite && (
          <div className="flex items-center gap-2 px-4 py-3 text-muted-foreground">
            <Search className="size-4 shrink-0" />
            No Entries for {site}. Type to search all of keefast.
          </div>
        )}
        {listed.length > 0 && (
          <CommandGroup heading={`For ${site}`}>
            {listed.map((entry) => (
              <EntryItem
                key={entry.uuid}
                entry={entry}
                filling={filling === entry.uuid}
                onSelect={() => void fill(entry.uuid, false)}
              />
            ))}
          </CommandGroup>
        )}
        {query.trim() && (
          <CommandGroup heading="Other Entries in keefast">
            {others.map((entry) => (
              <EntryItem
                key={entry.uuid}
                entry={entry}
                filling={filling === entry.uuid}
                onSelect={() => void fill(entry.uuid, true)}
              />
            ))}
            {others.length === 0 && (
              <p className="px-2 py-1.5 text-muted-foreground">
                {searching ? "Searching…" : "No other Entries match."}
              </p>
            )}
          </CommandGroup>
        )}
      </CommandList>
      <Footer site={site} />
    </Command>
  );
}

function EntryItem({
  entry,
  filling,
  onSelect,
}: {
  entry: EntrySummary | FoundEntry;
  filling: boolean;
  onSelect: () => void;
}) {
  return (
    <CommandItem value={entry.uuid} onSelect={onSelect} className="gap-3 py-2">
      {filling ? <LoaderCircle className="animate-spin" /> : <KeyRound />}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium">{entry.name || entry.username}</span>
          {entry.hasTotp && (
            <Badge variant="secondary" className="px-1.5 py-0 text-[10px]">
              TOTP
            </Badge>
          )}
        </div>
        {entry.username && (
          <div className="truncate text-xs text-muted-foreground">{entry.username}</div>
        )}
      </div>
      {entry.group && (
        <span className="max-w-[40%] truncate text-xs text-muted-foreground">{entry.group}</span>
      )}
    </CommandItem>
  );
}

/** Keys to use, and hiding keefast on the site: out of the list, so Enter never picks it. */
function Footer({ site }: { site: string }) {
  return (
    <div className="flex items-center gap-3 border-t px-3 py-1.5 text-xs text-muted-foreground">
      <Button
        variant="ghost"
        size="sm"
        className="-ml-2 h-7 px-2 text-xs text-muted-foreground"
        onClick={() => void ask({ type: "hide-site" })}
      >
        <EyeOff />
        Don't show on {site}
      </Button>
      <span className="ml-auto">
        <Key>↑</Key>
        <Key>↓</Key> to move
      </span>
      <span>
        <Key>Enter</Key> to fill
      </span>
      <span>
        <Key>Esc</Key> to close
      </span>
    </div>
  );
}

function Key({ children }: { children: string }) {
  return (
    <kbd className="mx-0.5 rounded border bg-muted px-1 font-sans text-[10px] text-foreground">
      {children}
    </kbd>
  );
}

/** A password from keefast's generator, shown for the user to fill or make another. */
function Generator({ site }: { site: string }) {
  const [password, setPassword] = useState<string>();
  const [notice, setNotice] = useState<Problem>();
  const [busy, setBusy] = useState(false);
  const fillButton = useRef<HTMLButtonElement>(null);

  const generate = useCallback(async () => {
    setBusy(true);
    setNotice(undefined);
    const answer = await ask({ type: "generate" });
    setBusy(false);
    if (answer.ok) {
      setPassword(answer.value);
      fillButton.current?.focus();
    } else if (answer.reason === "denied") {
      setNotice({
        title: "keefast's generator settings can't make a password.",
        detail: "Change them in keefast.",
      });
    } else {
      setNotice(problem(answer.reason));
    }
  }, []);

  useEffect(() => void generate(), [generate]);

  const fill = async () => {
    setBusy(true);
    const answer = await ask({ type: "fill-generated" });
    setBusy(false);
    // Once filled, keefast closes the palette.
    if (!answer.ok) setNotice(problem(answer.reason));
    else if (!answer.value) {
      setNotice({
        title: "Nothing was filled.",
        detail: "This page has no new password field any more, or it changed.",
      });
    }
  };

  return (
    <div className="space-y-3 p-4">
      <div>
        <p className="font-medium">New password for {site}</p>
        <p className="text-muted-foreground">Made with your generator settings in keefast.</p>
      </div>
      <Input
        readOnly
        value={password ?? ""}
        placeholder={busy ? "Generating…" : ""}
        aria-label="Generated password"
        spellCheck={false}
        className="font-mono"
        onFocus={(event) => event.currentTarget.select()}
      />
      {notice && (
        <div role="status">
          <p className="font-medium">{notice.title}</p>
          {notice.detail && <p className="text-muted-foreground">{notice.detail}</p>}
        </div>
      )}
      <div className="flex items-center gap-2">
        <Button ref={fillButton} disabled={busy || !password} onClick={() => void fill()}>
          Fill
        </Button>
        <Button variant="secondary" disabled={busy} onClick={() => void generate()}>
          <RefreshCw />
          Another
        </Button>
        <Button
          variant="ghost"
          className="ml-auto text-muted-foreground"
          onClick={() => void ask({ type: "hide-site" })}
        >
          <EyeOff />
          Don't show on {site}
        </Button>
      </div>
    </div>
  );
}
