// The mock backend: every command and event of the Tauri app, over the prototype's fake Vaults,
// in memory, so the UI runs in a normal browser (`pnpm dev:mock`). Crude where the real thing
// needs the OS (file dialogs answer with made-up paths, the clipboard is the browser's), but
// every command answers in the real one's shape, emits `workspaceChanged` after changes, and
// fails the way the real one does for the cases screens show (a wrong master key, a name taken).
//
// Driving it from the console or agent-browser, for states a screen must show:
//   __keefastMock.simulate("pairing" | "access" | "login" | "lockUnsaved" | "merge" | "saveFailed"
//     | "unlocked-none" | "closed" | "dropbox-disconnected" | "sync-offline" | "sync-error"
//     | "browserError" | "browserNone" | "browserServed" | "configError"
//     | "plugin-new" | "plugin-update" | "plugin-clash" | "plugin-key-changed" | "plugin-malformed"
//     | "plugin-keefast" | "plugin-fill" | "plugin-cancel" | "plugins-installed" | "plugins-dev-build")   (the Plugin scenes: ./plugin-store.ts)
//   __keefastMock.reset()
// or load the page with `?mock=<same names>` (comma-separated), and `?mock=unlocked-none` to
// start with every Vault locked; `?latency=300` makes commands slow and out of order. The unlock
// password is anything but "wrong".

import type {
  ApiError,
  AppSettings,
  CopyTarget,
  CustomFieldView,
  EncryptionSettings,
  EntryDetail,
  EntryInput,
  EntryQuery,
  EntrySummary,
  EntryVersion,
  ErrorKind,
  GeneratorOptions,
  GroupNode,
  IconView,
  Location,
  LocationView,
  LoginOption,
  LoginTarget,
  Scope,
  TotpSetup,
  View,
  WorkspaceSnapshot,
} from "@/bindings";

import type { Backend, Commands, EventPayloads, Events } from "../backend";
import {
  BRAND_COLORS,
  fakeRecents,
  fakeVaults,
  type MockEntry,
  type MockField,
  type MockGroup,
  type MockTotp,
  type MockVault,
} from "./data";
import { createMockPluginStore, PLUGIN_SCENES, PluginRefusal, type PluginScene } from "./plugin-store";
import { facetsOf, facetTemplates, formatOwned, listLabel, validateOwned } from "./plugins";

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: ApiError };

class MockError extends Error {
  constructor(
    readonly kind: ErrorKind,
    message: string,
  ) {
    super(message);
  }
}

const notFound = () => new MockError("notFound", "That Entry or Group no longer exists.");

/**
 * How long a command takes, in ms: a little, as IPC has, so loading states show. `?latency=300`
 * makes each command take up to that long, in any order, to shake out races with the backend.
 */
const latency = (() => {
  const asked = typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get("latency");
  const most = asked ? Number(asked) : NaN;
  return Number.isFinite(most) && most > 0 ? () => Math.random() * most : () => 8;
})();

/** A command that answers like the generated ones: never throws, resolves to a Result. */
function command<Args extends unknown[], T>(
  run: (...args: Args) => T | Promise<T>,
): (...args: Args) => Promise<Result<T>> {
  return async (...args) => {
    await new Promise((resolve) => setTimeout(resolve, latency()));
    try {
      return { status: "ok", data: await run(...args) };
    } catch (error) {
      if (error instanceof MockError) {
        return { status: "error", error: { kind: error.kind, message: error.message, detail: null } };
      }
      if (error instanceof PluginRefusal) {
        return { status: "error", error: { kind: "invalid", message: error.message, detail: error.detail } };
      }
      throw error;
    }
  };
}

const now = () => Math.floor(Date.now() / 1000);

/** A stand-in favicon: the prototype's tinted monogram tile. */
function monogram(entry: MockEntry): string | null {
  if (!entry.url) return null;
  const color = BRAND_COLORS[entry.brand] ?? "#3a3d45";
  const letter = (entry.title.trim()[0] ?? "?").toUpperCase().replace(/[<&>"]/g, "");
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="7" fill="${color}"/><text x="16" y="21.5" font-family="Inter,system-ui,sans-serif" font-size="15" font-weight="600" fill="#fff" text-anchor="middle">${letter}</text></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

/** A TOTP code that changes every period, as a real one does (not RFC 6238). */
function totpCode(totp: MockTotp, at = Date.now() / 1000) {
  const counter = Math.floor(at / totp.period);
  let hash = 2166136261;
  for (const char of `${totp.secret}:${counter}`) {
    hash = Math.imul(hash ^ char.charCodeAt(0), 16777619) >>> 0;
  }
  const code = String(hash % 10 ** totp.digits).padStart(totp.digits, "0");
  return { code, remaining: totp.period - (Math.floor(at) % totp.period), period: totp.period };
}

function locationView(location: Location): LocationView {
  const name = location.path.split("/").pop()!.replace(/\.[^.]+$/, "");
  return {
    ...location,
    name,
    description: location.provider === "dropbox" ? `Dropbox: ${location.path}` : location.path,
  };
}

/** The prototype's Dropbox: folders, and files with when they changed and their size. */
const today = (time: string) => `${new Date().toISOString().slice(0, 11)}${time}Z`;
const yesterday = (time: string) => `${new Date(Date.now() - 86_400_000).toISOString().slice(0, 11)}${time}Z`;
const DROPBOX_TREE: Record<string, [name: string, modified?: string, size?: number][]> = {
  "": [["Apps"], ["Documents"], ["Photos"], ["Shared"], ["Old-2019.kdbx", "2019-03-12T10:20:00Z", 65_536]],
  "/apps": [["keefast"], ["Obsidian"]],
  "/apps/keefast": [
    ["Archive"], ["Backups"], ["Shared with Ana"],
    ["Family.kdbx", yesterday("21:40:00"), 98_304],
    ["Old-laptop.kdbx", "2025-02-14T08:00:00Z", 58_368],
    ["Travel.kdbx", "2026-08-03T08:00:00Z", 24_576],
    ["Work.kdbx", today("09:12:00"), 217_088],
  ],
  "/apps/keefast/archive": [["Personal-2023.kdbx", "2023-12-31T18:00:00Z", 72_704]],
  "/apps/keefast/backups": [["Work-2026-09.kdbx", "2026-09-30T18:00:00Z", 212_992], ["Work-2026-08.kdbx", "2026-08-31T18:00:00Z", 205_824]],
  "/apps/keefast/shared with ana": [],
  "/apps/obsidian": [],
  "/documents": [["Taxes"], ["Household.kdbx", "2026-06-02T08:00:00Z", 38_912], ["Taxes 2025.pdf", "2026-04-10T08:00:00Z", 412_000]],
  "/documents/taxes": [],
  "/photos": [],
  "/shared": [["Team.kdbx", today("08:03:00"), 143_360]],
};

const DEFAULT_ENCRYPTION: EncryptionSettings = {
  cipher: "aes256",
  kdf: { kind: "argon2id", memoryKib: 65536, iterations: 5, parallelism: 4 },
};

const DEFAULT_GENERATOR: GeneratorOptions = {
  mode: "characters",
  characters: { length: 20, lowercase: true, uppercase: true, digits: true, symbols: true, excludeLookalikes: false },
  passphrase: { words: 6, separator: "-" },
};

const WORDS = "amber anchor apple arrow autumn badge banjo basil beacon birch bison blanket bramble breeze cabin canyon cedar chalk cinder clover comet copper coral cosmos crane dawn delta denim dune ember fable falcon fern fiddle flint forest fossil galaxy garnet ginger glacier granite harbor hazel heron hollow indigo island ivory jasper juniper kettle lantern lemon lilac lotus maple marble meadow mesa mint moss nectar nickel nova oak oasis olive onyx orbit otter pebble pepper pine plume prairie quartz quill raven reef ripple river saffron sage sequoia shadow slate sparrow spruce summit thistle thunder tulip tundra velvet violet walnut willow zephyr".split(" ");

function generate(options: GeneratorOptions): string {
  const random = (n: number) => crypto.getRandomValues(new Uint32Array(1))[0]! % n;
  if (options.mode === "passphrase") {
    const { words, separator } = options.passphrase;
    if (words < 1 || words > 40) throw new MockError("invalid", "The number of words must be between 1 and 40.");
    return Array.from({ length: words }, () => WORDS[random(WORDS.length)]).join(separator);
  }
  const c = options.characters;
  const lookalikes = "0O1Il|8B6G";
  const classes = [
    [c.lowercase, "abcdefghijklmnopqrstuvwxyz"],
    [c.uppercase, "ABCDEFGHIJKLMNOPQRSTUVWXYZ"],
    [c.digits, "0123456789"],
    [c.symbols, "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"],
  ] as const;
  const chosen = classes
    .filter(([on]) => on)
    .map(([, chars]) => [...chars].filter((ch) => !(c.excludeLookalikes && lookalikes.includes(ch))));
  if (chosen.length === 0) throw new MockError("invalid", "Choose at least one kind of character.");
  if (c.length < chosen.length || c.length > 256) {
    throw new MockError("invalid", `The length must be between ${chosen.length} and 256.`);
  }
  const all = chosen.flat();
  const out = chosen.map((chars) => chars[random(chars.length)]!);
  while (out.length < c.length) out.push(all[random(all.length)]!);
  for (let i = out.length - 1; i > 0; i--) {
    const j = random(i + 1);
    [out[i], out[j]] = [out[j]!, out[i]!];
  }
  return out.join("");
}

/** Where browser integration stands, as `get_settings` says it, by how registering went. */
const BROWSER_STATUS: Record<MockState["browserHosts"], AppSettings["browser"]> = {
  ok: { enabled: true, message: "Set up for Chrome, Brave", error: false, browsers: ["Chrome", "Brave"] },
  failed: {
    enabled: true,
    message: "Browser integration could not be set up: Permission denied (os error 13)",
    error: true,
    browsers: [],
  },
  none: { enabled: true, message: "No supported browser found (Chrome, Chromium, Brave, Edge, Vivaldi).", error: false, browsers: [] },
  served: { enabled: true, message: "Another keefast is already serving the browser.", error: true, browsers: [] },
};

interface MockState {
  vaults: MockVault[];
  recents: Location[];
  view: View;
  revision: number;
  settings: Omit<AppSettings, "browser" | "configError"> & { browserIntegration: boolean };
  pendingLock: { vault: number; canCancel: boolean } | null;
  pendingPairing: { id: number; vault: number } | null;
  pendingAccess: { id: number; site: string; entries: [number, string][] } | null;
  pendingLogin: { id: number; site: string; username: string; password: string; entries: [number, string][] } | null;
  clipboardClearsAt: number | null;
  dropboxAccount: { name: string; email: string } | null;
  /** Connecting Dropbox "in the browser": stops it as cancelled. */
  dropboxConnecting: (() => void) | null;
  /** Vaults' encryption, once changed from the default. */
  encryption: Map<number, EncryptionSettings>;
  /** How registering the browsers' native host went, while browser integration is on. */
  browserHosts: "ok" | "failed" | "none" | "served";
  /** Why the app config couldn't be read or saved. */
  configError: string | null;
  nextId: number;
}

function initialState(): MockState {
  return {
    vaults: fakeVaults(),
    recents: [...fakeRecents],
    view: { scope: { kind: "vault", vault: 0 }, entry: { vault: 0, entry: "e1" } },
    revision: 0,
    settings: {
      theme: "system",
      idleLockSeconds: 15 * 60,
      lockOnOsLock: true,
      clipboardClearSeconds: 30,
      autosave: true,
      browserIntegration: true,
      downloadFavicons: true,
      generator: structuredClone(DEFAULT_GENERATOR),
      paneWidths: { sidebar: 232, entryList: 360 },
    },
    pendingLock: null,
    pendingPairing: null,
    pendingAccess: null,
    pendingLogin: null,
    clipboardClearsAt: null,
    dropboxAccount: { name: "Arthur Rezende", email: "arthur-er@protonmail.com" },
    dropboxConnecting: null,
    encryption: new Map(),
    browserHosts: "ok",
    configError: null,
    nextId: 100,
  };
}

/** The scenes `?mock=` and `__keefastMock.simulate` set up. */
export type MockScene =
  | "pairing"
  | "access"
  | "login"
  | "lockUnsaved"
  | "merge"
  | "saveFailed"
  | "unlocked-none"
  | "closed"
  | "dropbox-disconnected"
  | "sync-offline"
  | "sync-error"
  | "browserError"
  | "browserNone"
  | "browserServed"
  | "configError"
  | PluginScene;

export function createMockBackend(): Backend & {
  simulate(scene: MockScene): void;
  reset(): void;
} {
  let state = initialState();
  let plugins = createMockPluginStore(changed);
  const listeners = new Map<keyof EventPayloads, Set<(event: { payload: never }) => void>>();

  function emit<Name extends keyof EventPayloads>(name: Name, payload: EventPayloads[Name]) {
    for (const listener of listeners.get(name) ?? []) {
      (listener as (event: { payload: EventPayloads[Name] }) => void)({ payload });
    }
  }

  /** After every change: a new revision, as the real backend's `update`. */
  function changed() {
    state.revision += 1;
    emit("workspaceChanged", { revision: state.revision });
  }

  function vault(id: number, unlocked = true): MockVault {
    const found = state.vaults.find((v) => v.id === id);
    if (!found || (unlocked && found.locked)) {
      throw new MockError("notFound", "That Vault is no longer open.");
    }
    return found;
  }

  function entryOf(vaultId: number, entryId: string): [MockVault, MockEntry] {
    const v = vault(vaultId);
    const e = v.entries.find((x) => x.id === entryId);
    if (!e) throw notFound();
    return [v, e];
  }

  function group(v: MockVault, id: string): MockGroup {
    const g = v.groups.find((x) => x.id === id);
    if (!g) throw notFound();
    return g;
  }

  function inBin(v: MockVault, groupId: string): boolean {
    for (let g: MockGroup | undefined = group(v, groupId); g; g = v.groups.find((x) => x.id === g!.parent)) {
      if (g.id === v.recycleBin) return true;
    }
    return false;
  }

  function descendants(v: MockVault, groupId: string): Set<string> {
    const out = new Set([groupId]);
    let grew = true;
    while (grew) {
      grew = false;
      for (const g of v.groups) {
        if (g.parent && out.has(g.parent) && !out.has(g.id)) {
          out.add(g.id);
          grew = true;
        }
      }
    }
    return out;
  }

  /** An edit: marks the Vault unsaved, then saves it shortly with autosave on, as the app does. */
  function edited(v: MockVault) {
    v.unsaved = true;
    changed();
    if (state.settings.autosave) save(v);
  }

  function save(v: MockVault) {
    v.saving = true;
    if (v.sync) v.sync = "syncing";
    setTimeout(() => {
      v.saving = false;
      v.unsaved = false;
      v.saveFailed = false;
      if (v.sync) v.sync = "synced";
      if (state.pendingLock?.vault === v.id) {
        state.pendingLock = null;
        v.locked = true;
      }
      changed();
    }, 400);
  }

  function snapshotVersion(e: MockEntry): MockEntry["history"][number] {
    return {
      modified: e.modified,
      title: e.title,
      username: e.username,
      password: e.password,
      url: e.url,
      notes: e.notes,
      tags: [...e.tags],
      color: e.color,
      fields: structuredClone(e.fields),
      totp: e.totp && { ...e.totp },
      attachments: e.attachments.map((a) => a.name),
    };
  }

  function keepHistory(e: MockEntry) {
    e.history.push(snapshotVersion(e));
    e.modified = now();
  }

  function icon(v: MockVault, e: MockEntry): IconView {
    if (e.icon.kind === "custom") {
      const id = e.icon.id;
      const custom = v.customIcons.find((c) => c.id === id);
      if (custom) return { kind: "custom", id, url: custom.png };
      return { kind: "standard", number: 0 };
    }
    return e.icon;
  }

  function summary(v: MockVault, e: MockEntry): EntrySummary {
    return {
      vault: v.id,
      id: e.id,
      title: e.title.trim() || "(untitled)",
      username: e.username,
      label: null,
      url: e.url,
      tags: e.tags,
      color: e.color,
      icon: icon(v, e),
      faviconUrl: state.settings.downloadFavicons ? monogram(e) : null,
      expires: e.expires,
      expired: e.expires !== null && e.expires <= now(),
      created: e.created,
      modified: e.modified,
      hasPassword: e.password !== "",
      hasTotp: e.totp !== null,
      attachmentCount: e.attachments.length,
      group: e.group,
      groupName: group(v, e.group).name,
      vaultName: v.name,
      inRecycleBin: inBin(v, e.group),
    };
  }

  /** The summary with the subtitle a Plugin derives when the Entry has no username. */
  async function labelled(s: EntrySummary, e: MockEntry): Promise<EntrySummary> {
    return s.username !== "" ? s : { ...s, label: await listLabel(e.fields, plugins.runs) };
  }

  function fieldViews(fields: MockEntry["fields"]): CustomFieldView[] {
    return fields.map((f) => ({
      name: f.name,
      protected: f.protected,
      value: f.protected ? null : f.value,
      isEmpty: f.value === "",
      // The mock keeps TOTP apart from the fields (`MockEntry.totp`), so none holds it.
      totpStorage: false,
    }));
  }

  function groupPath(v: MockVault, groupId: string): string[] {
    const path: string[] = [];
    for (let g: MockGroup | undefined = group(v, groupId); g; g = v.groups.find((x) => x.id === g!.parent)) {
      path.unshift(g.name);
    }
    return path;
  }

  function groupNode(v: MockVault, g: MockGroup, bin: boolean): GroupNode {
    const isBin = g.id === v.recycleBin;
    const below = descendants(v, g.id);
    return {
      id: g.id,
      name: g.name,
      entryCount: v.entries.filter((e) => e.group === g.id).length,
      totalEntries: v.entries.filter((e) => below.has(e.group)).length,
      isRecycleBin: isBin,
      inRecycleBin: bin || isBin,
      isBrowserGroup: v.browserGroup === g.id,
      children: v.groups.filter((c) => c.parent === g.id).map((c) => groupNode(v, c, bin || isBin)),
    };
  }

  function readable(): MockVault[] {
    return state.vaults.filter((v) => !v.locked && state.pendingLock?.vault !== v.id);
  }

  function scoped(scope: Scope): [MockVault, MockEntry][] {
    const outsideBin = (v: MockVault) =>
      v.entries.filter((e) => !inBin(v, e.group)).map((e) => [v, e] as [MockVault, MockEntry]);
    switch (scope.kind) {
      case "allVaults":
        return readable().flatMap((v) => v.entries.map((e) => [v, e] as [MockVault, MockEntry]));
      case "vault":
        return readable()
          .filter((v) => v.id === scope.vault)
          .flatMap((v) => v.entries.map((e) => [v, e] as [MockVault, MockEntry]));
      case "group":
        return readable()
          .filter((v) => v.id === scope.vault)
          .flatMap((v) => {
            const below = descendants(v, scope.group);
            return v.entries.filter((e) => below.has(e.group)).map((e) => [v, e] as [MockVault, MockEntry]);
          });
      case "tag":
        return readable().flatMap(outsideBin).filter(([, e]) => e.tags.includes(scope.tag));
      case "color":
        return readable().flatMap(outsideBin).filter(([, e]) => e.color === scope.color);
    }
  }

  function matches(e: MockEntry, words: string[]): number | null {
    let score = 0;
    for (const word of words) {
      const fields: [string, number][] = [
        [e.title, 4],
        [e.username, 3],
        [e.url, 3],
        [e.tags.join(" "), 2],
        [e.notes, 1],
        [e.fields.filter((f) => !f.protected).map((f) => f.value).join(" "), 1],
      ];
      const best = Math.max(
        ...fields.map(([text, weight]) => {
          const at = text.toLowerCase().indexOf(word);
          return at < 0 ? 0 : weight * (at === 0 ? 2 : 1);
        }),
      );
      if (best === 0) return null;
      score += best;
    }
    return score;
  }

  function sortKey(e: MockEntry, sort: EntryQuery["sort"]): string | number {
    switch (sort) {
      case "title":
        return e.title.toLowerCase() || "￿";
      case "username":
        return e.username.toLowerCase() || "￿";
      case "url":
        return e.url.toLowerCase() || "￿";
      case "created":
        return -e.created;
      case "modified":
        return -e.modified;
    }
  }

  /** The Entries of the readable Vaults for `site`, with `username` if given, as the backend
   * finds the Entries a request is about. */
  function entriesFor(site: string, username?: string): [number, string][] {
    return readable().flatMap((v) =>
      v.entries
        .filter((e) => !inBin(v, e.group) && siteOf(e.url) === site && (username === undefined || e.username === username))
        .map((e): [number, string] => [v.id, e.id]),
    );
  }

  function siteOf(url: string): string {
    try {
      return new URL(url).host;
    } catch {
      return "";
    }
  }

  function loginOptions(login: NonNullable<MockState["pendingLogin"]>): LoginOption[] {
    const options: LoginOption[] = [];
    const updates = login.entries.flatMap(([vaultId, entryId]) => {
      const v = readable().find((x) => x.id === vaultId);
      const e = v?.entries.find((x) => x.id === entryId);
      return v && e ? [{ v, e }] : [];
    });
    if (updates.length > 0) options.push({ target: null, label: "Update an existing Entry", detail: null, depth: 0 });
    for (const { v, e } of updates) {
      options.push({
        target: { kind: "update", vault: v.id, entry: e.id },
        label: `${e.title || "(untitled)"} · ${e.username}`,
        detail: v.name,
        depth: 1,
      });
    }
    options.push({ target: null, label: "Save as a new Entry in", detail: null, depth: 0 });
    for (const v of readable()) {
      const root = v.groups.find((g) => g.parent === null)!;
      options.push({ target: { kind: "newEntry", vault: v.id, group: root.id }, label: v.name, detail: null, depth: 1 });
      if (!v.browserGroup) {
        options.push({ target: { kind: "newEntry", vault: v.id, group: null }, label: "Browser (new Group)", detail: "Browser group", depth: 2 });
      }
      const walk = (parent: string, depth: number) => {
        for (const g of v.groups.filter((x) => x.parent === parent)) {
          if (inBin(v, g.id)) continue;
          options.push({
            target: { kind: "newEntry", vault: v.id, group: g.id },
            label: g.name,
            detail: v.browserGroup === g.id ? "Browser group" : null,
            depth,
          });
          walk(g.id, depth + 1);
        }
      };
      walk(root.id, 2);
    }
    return options;
  }

  function snapshot(): WorkspaceSnapshot {
    const tags = new Set<string>();
    for (const v of readable()) for (const e of v.entries) if (!inBin(v, e.group)) e.tags.forEach((t) => tags.add(t));
    const locked = state.vaults.filter((v) => v.locked);
    const shown = state.view.scope.kind === "vault" || state.view.scope.kind === "group" ? state.view.scope.vault : null;
    const login = state.pendingLogin;
    return {
      revision: state.revision,
      vaults: state.vaults.map((v) => {
        const locking = state.pendingLock?.vault === v.id;
        const hidden = v.locked || locking;
        return {
          id: v.id,
          name: v.name,
          location: locationView(v.location),
          state: v.locked ? "locked" : locking ? "locking" : "unlocked",
          unsavedChanges: !v.locked && v.unsaved,
          saving: !v.locked && v.saving,
          saveFailed: !v.locked && v.saveFailed,
          syncStatus: v.locked ? null : v.sync,
          entryCount: hidden ? 0 : v.entries.length,
          rootGroup: hidden ? null : v.groups.find((g) => g.parent === null)!.id,
          recycleBin: hidden ? null : v.recycleBin,
          browserGroup: hidden ? null : v.browserGroup,
        };
      }),
      view: state.view,
      saveStatus: state.vaults.some((v) => !v.locked && v.saving)
        ? "saving"
        : state.vaults.some((v) => !v.locked && v.saveFailed)
          ? "failed"
          : state.vaults.some((v) => !v.locked && v.unsaved)
            ? "unsaved"
            : "saved",
      vaultToUnlock: locked.find((v) => v.id === shown)?.id ?? locked[0]?.id ?? null,
      recentVaults: state.recents.map(locationView),
      tags: [...tags].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" })),
      pendingLock: state.pendingLock && {
        vault: state.pendingLock.vault,
        vaultName: vault(state.pendingLock.vault, false).name,
        canCancel: state.pendingLock.canCancel,
      },
      pendingPairing: state.pendingPairing && {
        id: state.pendingPairing.id,
        vault: state.pendingPairing.vault,
        vaultName: vault(state.pendingPairing.vault, false).name,
      },
      pendingAccess: state.pendingAccess && {
        id: state.pendingAccess.id,
        site: state.pendingAccess.site,
        entries: state.pendingAccess.entries.flatMap(([vaultId, entryId]) => {
          const v = state.vaults.find((x) => x.id === vaultId);
          const e = v?.entries.find((x) => x.id === entryId);
          return v && e ? [{ vault: v.id, entry: e.id, title: e.title, username: e.username, vaultName: v.name }] : [];
        }),
      },
      pendingLogin: login && {
        id: login.id,
        site: login.site,
        username: login.username,
        options: loginOptions(login),
        // As the backend: update the Entry it matched, else a new Entry in the Browser group.
        suggested: login.entries[0]
          ? { kind: "update", vault: login.entries[0][0], entry: login.entries[0][1] }
          : { kind: "newEntry", vault: readable()[0]?.id ?? 0, group: readable()[0]?.browserGroup ?? null },
      },
      clipboardClearsIn: state.clipboardClearsAt === null ? null : Math.max(0, state.clipboardClearsAt - Date.now() / 1000),
      dropboxAvailable: true,
      configError: state.configError,
    };
  }

  function fieldsFrom(input: EntryInput, current: MockEntry | null): Partial<MockEntry> {
    const fields = input.customFields.map((f) => {
      const kept = f.original !== null ? current?.fields[f.original] : undefined;
      if (f.value === null && f.original !== null && !kept) throw notFound();
      if (!f.name.trim()) throw new MockError("invalid", "Give every custom field a name.");
      return { name: f.name.trim(), value: f.value ?? kept?.value ?? "", protected: f.protected };
    });
    const names = fields.map((f) => f.name);
    if (new Set(names).size !== names.length || names.some((n) => ["Title", "UserName", "Password", "URL", "Notes"].includes(n))) {
      throw new MockError(
        "invalid",
        "Each field needs a name of its own, other than Title, UserName, Password, URL or Notes.",
      );
    }
    let expires: number | null = null;
    if (input.expires !== null) {
      const parsed = /^\d{4}-\d{2}-\d{2}$/.test(input.expires.trim()) ? Date.parse(`${input.expires.trim()}T00:00:00Z`) : NaN;
      if (Number.isNaN(parsed)) throw new MockError("invalid", "Type the expiry date as YYYY-MM-DD, such as 2027-01-31.");
      expires = parsed / 1000;
    }
    return {
      title: input.title,
      username: input.username,
      password: input.password ?? current?.password ?? "",
      url: input.url,
      notes: input.notes,
      fields,
      tags: input.tags.flatMap((t) => t.split(/[,;]/)).map((t) => t.trim()).filter(Boolean),
      color: input.color,
      icon: input.icon,
      expires,
    };
  }

  function requireName(name: string) {
    if (!name.trim()) throw new MockError("invalid", "Give this browser a name.");
  }

  function lockVaults(ids: number[], canCancel: boolean) {
    for (const id of ids) {
      const v = vault(id);
      if (v.unsaved) {
        if (state.settings.autosave) save(v);
        state.pendingLock ??= { vault: v.id, canCancel };
      } else {
        v.locked = true;
      }
    }
    if (state.clipboardClearsAt !== null) {
      state.clipboardClearsAt = null;
      emit("clipboardCleared", {});
    }
    changed();
  }

  let clipboardTimer: ReturnType<typeof setTimeout> | undefined;

  const commands: Commands = {
    getWorkspace: command(() => snapshot()),
    getLaunch: command(() => ({ vaultFile: null, keyFile: null })),
    setView: command((view: View) => {
      state.view = view;
      changed();
      return state.revision;
    }),
    noteActivity: command(() => null),
    groupForNewItems: command((scope: Scope) => {
      const v =
        scope.kind === "vault" || scope.kind === "group"
          ? vault(scope.vault)
          : readable().length === 1
            ? readable()[0]!
            : null;
      if (!v) return null;
      const g = scope.kind === "group" ? scope.group : v.groups.find((x) => x.parent === null)!.id;
      return { vault: v.id, group: g };
    }),
    defaultEncryption: command(() => structuredClone(DEFAULT_ENCRYPTION)),
    // The prototype's rates: no key derivation runs in the browser.
    estimateUnlockTime: command(({ kdf }: EncryptionSettings) =>
      kdf.kind === "aesKdf" ? kdf.rounds / 2.4e6 : (kdf.memoryKib / 1024) * kdf.iterations / Math.max(kdf.parallelism, 1) / 420,
    ),
    listPlugins: command(() => plugins.list()),
    choosePluginPackage: command(() => plugins.choose()),
    installPlugin: command((path: string, digest: string) => plugins.install(path, digest)),
    setPluginEnabled: command((id: string, enabled: boolean) => plugins.setEnabled(id, enabled)),
    removePlugin: command((id: string) => plugins.remove(id)),
    chooseVaultFile: command(() => "~/Vaults/Picked.kdbx"),
    chooseKeyFile: command(() => "~/Vaults/Personal.keyx"),
    chooseNewVaultFile: command(() => "~/Vaults/New Vault.kdbx"),
    createKeyFile: command((near: string | null) => {
      const folder = near?.includes("/") ? near.slice(0, near.lastIndexOf("/")) : "~/Vaults";
      return `${folder}/Vault.keyx`;
    }),
    unlockVault: command(async (location: Location, key) => {
      await new Promise((resolve) => setTimeout(resolve, 600));
      if (key.password === "wrong") throw new MockError("wrongKey", "Wrong master password or Key file.");
      if (!key.password && !key.keyFile) throw new MockError("wrongKey", "Wrong master password or Key file.");
      const existing = state.vaults.find((v) => v.location.path === location.path);
      if (existing) {
        existing.locked = false;
        state.view = { scope: { kind: "vault", vault: existing.id }, entry: null };
        changed();
        return existing.id;
      }
      // A fake Vault closed by the "closed" scene opens with its Entries again.
      const reopened = fakeVaults().find((v) => v.location.path === location.path);
      if (reopened && !state.vaults.some((v) => v.id === reopened.id)) {
        state.vaults.push({ ...reopened, locked: false });
        state.vaults.sort((x, y) => x.id - y.id);
        state.recents = state.recents.filter((r) => r.path !== location.path);
        state.view = { scope: { kind: "vault", vault: reopened.id }, entry: null };
        changed();
        return reopened.id;
      }
      if (location.path.includes("Old-2019")) {
        // The prototype's missing Vault file.
        throw new MockError("notFound", "The Vault file could not be found.");
      }
      const id = Math.max(-1, ...state.vaults.map((v) => v.id)) + 1;
      const name = locationView(location).name;
      state.vaults.push({
        id, name, location, locked: false,
        groups: [{ id: "root", name, parent: null }, { id: "bin", name: "Recycle Bin", parent: "root" }],
        entries: [], recycleBin: "bin", browserGroup: null, unsaved: false, saving: false, saveFailed: false,
        sync: location.provider === "dropbox" ? "synced" : null, pairings: [], policy: "ask", customIcons: [],
      });
      state.recents = state.recents.filter((r) => r.path !== location.path);
      state.view = { scope: { kind: "vault", vault: id }, entry: null };
      changed();
      return id;
    }),
    createVault: command(async (input) => {
      if (!input.path.trim()) throw new MockError("invalid", "Choose where to save the Vault file.");
      if (!input.key.password && !input.key.keyFile) throw new MockError("invalid", "Set a master password, a Key file, or both.");
      if (state.vaults.some((v) => !v.locked && v.location.path === input.path)) {
        throw new MockError("alreadyOpen", "A Vault from this Vault file is open. Choose another file, or lock and reopen that Vault.");
      }
      // The Vault files the mock knows exist, locked or recent.
      if ([...state.vaults.map((v) => v.location), ...state.recents].some((l) => l.provider === "local" && l.path === input.path)) {
        throw new MockError("conflict", "A file is already there. keefast never replaces a file with a new Vault.");
      }
      await new Promise((resolve) => setTimeout(resolve, 600));
      const id = Math.max(-1, ...state.vaults.map((v) => v.id)) + 1;
      const name = input.name.trim() || locationView({ provider: "local", path: input.path }).name;
      state.vaults.push({
        id, name, location: { provider: "local", path: input.path }, locked: false,
        groups: [{ id: "root", name, parent: null }], entries: [], recycleBin: null, browserGroup: null,
        unsaved: false, saving: false, saveFailed: false, sync: null, pairings: [], policy: "ask", customIcons: [],
      });
      state.view = { scope: { kind: "vault", vault: id }, entry: null };
      changed();
      return id;
    }),
    forgetRecent: command((location: Location) => {
      state.recents = state.recents.filter((r) => r.path !== location.path);
      changed();
      return null;
    }),
    lockAll: command(() => {
      lockVaults(state.vaults.filter((v) => !v.locked).map((v) => v.id), true);
      return null;
    }),
    lockVault: command((id) => {
      lockVaults([vault(id).id], true);
      return null;
    }),
    answerLock: command((id, answer) => {
      const v = vault(id);
      if (state.pendingLock?.vault !== id) return null;
      if (answer === "cancel") {
        if (state.pendingLock.canCancel) state.pendingLock = null;
      } else if (answer === "discard") {
        state.pendingLock = null;
        v.unsaved = false;
        v.locked = true;
      } else {
        save(v);
      }
      changed();
      return null;
    }),
    saveAll: command(() => {
      for (const v of state.vaults) if (!v.locked && (v.unsaved || v.saveFailed)) save(v);
      changed();
      return null;
    }),
    saveVault: command((id) => {
      save(vault(id));
      changed();
      return null;
    }),
    syncNow: command(() => {
      for (const v of state.vaults) if (!v.locked && v.sync) save(v);
      changed();
      return null;
    }),
    listGroups: command((id) => {
      const v = vault(id);
      return groupNode(v, v.groups.find((g) => g.parent === null)!, false);
    }),
    createGroup: command((id, parent, name) => {
      const v = vault(id);
      group(v, parent);
      const made = `g${state.nextId++}`;
      v.groups.push({ id: made, name, parent });
      edited(v);
      return made;
    }),
    renameGroup: command((id, groupId, name) => {
      const v = vault(id);
      group(v, groupId).name = name;
      edited(v);
      return null;
    }),
    moveGroup: command((id, groupId, to) => {
      const v = vault(id);
      const g = group(v, groupId);
      if (g.parent === null) throw new MockError("invalid", "The top Group of a Vault cannot be moved or deleted.");
      if (descendants(v, groupId).has(to)) throw new MockError("invalid", "A Group cannot be moved into itself or its subgroups.");
      group(v, to);
      g.parent = to;
      edited(v);
      return null;
    }),
    deleteGroup: command((id, groupId) => {
      const v = vault(id);
      const g = group(v, groupId);
      if (g.parent === null) throw new MockError("invalid", "The top Group of a Vault cannot be moved or deleted.");
      if (v.recycleBin && !inBin(v, groupId)) {
        g.parent = v.recycleBin;
      } else {
        const gone = descendants(v, groupId);
        v.groups = v.groups.filter((x) => !gone.has(x.id));
        v.entries = v.entries.filter((e) => !gone.has(e.group));
      }
      edited(v);
      return null;
    }),
    deletesGroupForGood: command((id, groupId) => {
      const v = vault(id);
      return !v.recycleBin || inBin(v, groupId);
    }),
    setBrowserGroup: command((id, groupId) => {
      const v = vault(id);
      if (inBin(v, groupId)) throw new MockError("invalid", "Logins can't be saved in the Recycle Bin.");
      v.browserGroup = groupId;
      edited(v);
      return null;
    }),
    listEntries: command(async (query: EntryQuery) => {
      const words = query.search.toLowerCase().split(/\s+/).filter(Boolean);
      const listed = scoped(query.scope)
        .map(([v, e]) => ({ v, e, score: matches(e, words) }))
        .filter((x): x is { v: MockVault; e: MockEntry; score: number } => x.score !== null)
        .sort((a, b) => {
          if (b.score !== a.score) return b.score - a.score;
          const [ka, kb] = [sortKey(a.e, query.sort), sortKey(b.e, query.sort)];
          return ka < kb ? -1 : ka > kb ? 1 : 0;
        })
        .map(({ v, e }) => labelled(summary(v, e), e));
      return Promise.all(listed);
    }),
    getEntry: command(async (id, entryId): Promise<EntryDetail> => {
      const [v, e] = entryOf(id, entryId);
      return {
        ...(await labelled(summary(v, e), e)),
        rawTitle: e.title,
        notes: e.notes,
        additionalUrls: [],
        customFields: fieldViews(e.fields),
        facets: await facetsOf(e.fields, plugins.runs),
        attachments: e.attachments.map((a, index) => ({
          index,
          name: a.name,
          size: a.size,
          preview: a.text !== undefined ? "text" : a.image !== undefined ? "image" : "none",
        })),
        totp: e.totp && { status: "ready", digits: e.totp.digits, period: e.totp.period, algorithm: e.totp.algorithm, steam: false },
        historyCount: e.history.length,
        rememberedAccess: e.access,
        groupPath: groupPath(v, e.group),
        deletesForGood: !v.recycleBin || inBin(v, e.group),
      };
    }),
    listFacets: command(() => facetTemplates(plugins.runs)),
    formatOwnedField: command(async (plugin, facet, key, value) => {
      const formatted = await formatOwned(plugin, facet, key, value, plugins.runs);
      if (formatted === null) throw new MockError("notFound", "That Plugin isn't installed.");
      return formatted;
    }),
    validateOwnedField: command(async (plugin, facet, key, value) => {
      const problem = await validateOwned(plugin, facet, key, value, plugins.runs);
      if (problem === undefined) throw new MockError("notFound", "That Plugin isn't installed.");
      return problem;
    }),
    revealPassword: command((id, entryId) => entryOf(id, entryId)[1].password),
    revealCustomField: command((id, entryId, index) => {
      const field = entryOf(id, entryId)[1].fields[index];
      if (!field) throw notFound();
      return field.value;
    }),
    createEntry: command((id, groupId, input) => {
      const v = vault(id);
      group(v, groupId);
      const made: MockEntry = {
        id: `n${state.nextId++}`, title: "", brand: "", username: "", password: "", url: "", notes: "", tags: [],
        color: null, icon: { kind: "standard", number: 0 }, totp: null, attachments: [], fields: [], expires: null,
        created: now(), modified: now(), group: groupId, history: [], access: [],
        ...fieldsFrom(input, null),
      };
      v.entries.push(made);
      edited(v);
      return made.id;
    }),
    updateEntry: command((id, entryId, input) => {
      const [v, e] = entryOf(id, entryId);
      const next = fieldsFrom(input, e);
      keepHistory(e);
      Object.assign(e, next);
      edited(v);
      return null;
    }),
    moveEntry: command((id, entryId, to) => {
      const [v, e] = entryOf(id, entryId);
      group(v, to);
      e.group = to;
      edited(v);
      return null;
    }),
    deleteEntry: command((id, entryId) => {
      const [v, e] = entryOf(id, entryId);
      if (v.recycleBin && !inBin(v, e.group)) e.group = v.recycleBin;
      else v.entries = v.entries.filter((x) => x !== e);
      if (state.view.entry?.entry === entryId) state.view = { ...state.view, entry: null };
      edited(v);
      return null;
    }),
    openEntryUrl: command((id, entryId) => {
      const url = entryOf(id, entryId)[1].url.trim();
      const web = /^https?:\/\/\S+$/i.test(url) || (/^[^\s:/]+(:\d+)?(\/\S*)?$/.test(url) && url !== "");
      if (!web) throw new MockError("invalid", "keefast opens only web addresses, starting with http or https.");
      // The mock doesn't open tabs: screenshots and tests would collect them.
      console.info(`keefast mock: would open ${url} in the browser`);
      return null;
    }),
    forgetAccess: command((id, entryId, host) => {
      const [v, e] = entryOf(id, entryId);
      e.access = host === null ? [] : e.access.filter((a) => a.host !== host);
      edited(v);
      return null;
    }),
    getEntryHistory: command((id, entryId): EntryVersion[] => {
      const [, e] = entryOf(id, entryId);
      // The protected fields both have whose values differ, as the backend tells them.
      const protectedChanges = (a: MockField[], b: MockField[]) =>
        a
          .filter((field) => {
            const other = b.find((o) => o.name === field.name);
            return other !== undefined && (field.protected || other.protected) && field.value !== other.value;
          })
          .map((field) => field.name);
      return e.history.map((version, index) => {
        const previous = e.history[index - 1];
        return {
          index,
          modified: version.modified,
          title: version.title || "(untitled)",
          username: version.username,
          url: version.url,
          notes: version.notes,
          hasPassword: version.password !== "",
          passwordIsCurrent: version.password === e.password,
          passwordIsPrevious: previous !== undefined && version.password === previous.password,
          customFields: fieldViews(version.fields),
          tags: version.tags,
          color: version.color,
          totp: version.totp ? "set" : "none",
          attachments: version.attachments,
          protectedChangedFromPrevious: previous ? protectedChanges(version.fields, previous.fields) : [],
          protectedChangedFromCurrent: protectedChanges(version.fields, e.fields),
        };
      });
    }),
    revealVersionPassword: command((id, entryId, index) => {
      const version = entryOf(id, entryId)[1].history[index];
      if (!version) throw new MockError("notFound", "That version is no longer in the Entry's history.");
      return version.password;
    }),
    revealVersionCustomField: command((id, entryId, index, field) => {
      const value = entryOf(id, entryId)[1].history[index]?.fields[field]?.value;
      if (value === undefined) throw notFound();
      return value;
    }),
    restoreEntryVersion: command((id, entryId, index) => {
      const [v, e] = entryOf(id, entryId);
      const version = e.history[index];
      if (!version) throw new MockError("invalid", "That version is no longer in the Entry's history.");
      keepHistory(e);
      Object.assign(e, { ...structuredClone(version), modified: now(), attachments: e.attachments });
      edited(v);
      return null;
    }),
    getTotpCode: command((id, entryId) => {
      const totp = entryOf(id, entryId)[1].totp;
      return totp && totpCode(totp);
    }),
    revealTotpSetup: command((id, entryId) => {
      const totp = entryOf(id, entryId)[1].totp;
      return totp && { secretOrUri: totp.secret, algorithm: totp.algorithm, digits: String(totp.digits), period: String(totp.period) };
    }),
    previewTotp: command((setup: TotpSetup) => totpCode(parseTotp(setup))),
    setTotp: command((id, entryId, setup) => {
      const [v, e] = entryOf(id, entryId);
      const totp = parseTotp(setup);
      keepHistory(e);
      e.totp = totp;
      edited(v);
      return null;
    }),
    removeTotp: command((id, entryId) => {
      const [v, e] = entryOf(id, entryId);
      keepHistory(e);
      e.totp = null;
      edited(v);
      return null;
    }),
    addAttachments: command((id, entryId) => {
      const [v, e] = entryOf(id, entryId);
      let name = "notes.txt";
      for (let n = 2; e.attachments.some((a) => a.name === name); n++) name = `notes (${n}).txt`;
      keepHistory(e);
      e.attachments.push({ name, text: "Attached in the mock backend.", size: 29 });
      edited(v);
      return { added: [name], errors: [] };
    }),
    removeAttachment: command((id, entryId, index) => {
      const [v, e] = entryOf(id, entryId);
      if (!e.attachments[index]) throw notFound();
      keepHistory(e);
      e.attachments.splice(index, 1);
      edited(v);
      return null;
    }),
    previewAttachment: command((id, entryId, index) => {
      const attachment = entryOf(id, entryId)[1].attachments[index];
      if (!attachment) throw notFound();
      if (attachment.text !== undefined) return { kind: "text" as const, text: attachment.text, cut: false };
      if (attachment.image !== undefined) return { kind: "image" as const, dataUrl: attachment.image };
      return { kind: "none" as const };
    }),
    saveAttachment: command((id, entryId, index) => {
      const attachment = entryOf(id, entryId)[1].attachments[index];
      if (!attachment) throw notFound();
      return `~/Downloads/${attachment.name}`;
    }),
    listCustomIcons: command((id) => vault(id).customIcons.map((c) => ({ id: c.id, url: c.png }))),
    importCustomIcon: command((id) => {
      const v = vault(id);
      const made = `i${state.nextId++}`;
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><circle cx="16" cy="16" r="14" fill="#a970ff"/></svg>`;
      v.customIcons.push({ id: made, png: `data:image/svg+xml,${encodeURIComponent(svg)}` });
      edited(v);
      return made;
    }),
    copyField: command((id, entryId, field: CopyTarget) => {
      const [, e] = entryOf(id, entryId);
      const text =
        field.kind === "username" ? e.username
        : field.kind === "password" ? e.password
        : field.kind === "url" ? e.url
        : field.kind === "totp" ? (e.totp ? totpCode(e.totp).code : "")
        : (e.fields[field.index]?.value ?? "");
      if (!text) throw new MockError("empty", "That field is empty, so nothing was copied.");
      void navigator.clipboard?.writeText(text).catch(() => {});
      const seconds = state.settings.clipboardClearSeconds;
      clearTimeout(clipboardTimer);
      state.clipboardClearsAt = seconds === null ? null : Date.now() / 1000 + seconds;
      if (seconds !== null) {
        clipboardTimer = setTimeout(() => {
          state.clipboardClearsAt = null;
          emit("clipboardCleared", {});
          changed();
        }, seconds * 1000);
      }
      changed();
      return { clearsInSeconds: seconds };
    }),
    generatorLimits: command(() => ({ maxLength: 256, maxWords: 40, symbols: "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~", lookalikes: "0O1Il|8B6G" })),
    setGeneratorSettings: command((settings: GeneratorOptions) => {
      state.settings.generator = structuredClone(settings);
      changed();
      return null;
    }),
    generatePassword: command((settings) => generate(settings ?? state.settings.generator)),
    getSettings: command((): AppSettings => {
      const { browserIntegration, ...settings } = state.settings;
      return {
        ...structuredClone(settings),
        browser: browserIntegration ? BROWSER_STATUS[state.browserHosts] : { enabled: false, message: null, error: false, browsers: [] },
        configError: state.configError,
      };
    }),
    setTheme: command((theme) => {
      state.settings.theme = theme;
      changed();
      return null;
    }),
    setIdleLockAfter: command((seconds) => {
      state.settings.idleLockSeconds = seconds || null;
      changed();
      return null;
    }),
    setLockOnOsLock: command((enabled) => {
      state.settings.lockOnOsLock = enabled;
      changed();
      return null;
    }),
    setClipboardClearAfter: command((seconds) => {
      state.settings.clipboardClearSeconds = seconds || null;
      changed();
      return null;
    }),
    setAutosave: command((enabled) => {
      state.settings.autosave = enabled;
      changed();
      return null;
    }),
    setBrowserIntegration: command((enabled) => {
      state.settings.browserIntegration = enabled;
      // Turned on again, the native host registers anew: as the real one, it works this time.
      if (enabled) state.browserHosts = "ok";
      changed();
      return null;
    }),
    setDownloadFavicons: command((enabled) => {
      state.settings.downloadFavicons = enabled;
      changed();
      return null;
    }),
    setPaneWidths: command((widths) => {
      state.settings.paneWidths = widths;
      return null;
    }),
    getVaultSettings: command((id) => {
      const v = vault(id);
      return {
        vault: v.id,
        name: v.name,
        location: locationView(v.location),
        saveStatus: v.saving ? "saving" : v.saveFailed ? "failed" : v.unsaved ? "unsaved" : "saved",
        syncStatus: v.sync,
        encryption: structuredClone(state.encryption.get(v.id) ?? DEFAULT_ENCRYPTION),
        defaultAccessPolicy: v.policy,
        pairings: [...v.pairings],
        browserGroup: v.browserGroup,
        browserGroupName: v.browserGroup ? group(v, v.browserGroup).name : null,
      };
    }),
    changeVaultKey: command((id, input) => {
      const v = vault(id);
      if (input.newKey && !input.newKey.password && !input.newKey.keyFile) {
        throw new MockError("invalid", "Set a master password, a Key file, or both.");
      }
      const kdf = input.encryption.kdf;
      if (kdf.kind !== "aesKdf" && kdf.iterations < 1) throw new MockError("invalid", "Argon2 needs at least 1 iteration.");
      if (kdf.kind === "aesKdf" && kdf.rounds < 1) throw new MockError("invalid", "AES-KDF needs at least 1 round.");
      state.encryption.set(v.id, structuredClone(input.encryption));
      v.unsaved = true;
      save(v);
      changed();
      return null;
    }),
    setDefaultAccessPolicy: command((id, policy) => {
      const v = vault(id);
      v.policy = policy;
      edited(v);
      return null;
    }),
    revokePairing: command((id, name) => {
      const v = vault(id);
      if (!v.pairings.includes(name)) throw new MockError("invalid", "This browser is no longer paired with the Vault.");
      v.pairings = v.pairings.filter((p) => p !== name);
      edited(v);
      return null;
    }),
    dropboxStatus: command(() => ({ available: true, account: state.dropboxAccount, connecting: state.dropboxConnecting !== null })),
    dropboxConnect: command(async () => {
      // "Allowed in the browser" after a few seconds, unless stopped first.
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(resolve, 4000);
        state.dropboxConnecting = () => {
          clearTimeout(timer);
          reject(new MockError("cancelled", "Connecting Dropbox was stopped."));
        };
      }).finally(() => (state.dropboxConnecting = null));
      state.dropboxAccount = { name: "Arthur Rezende", email: "arthur-er@protonmail.com" };
      return state.dropboxAccount;
    }),
    dropboxReopenConnectPage: command(() => null),
    dropboxCancelConnect: command(() => {
      state.dropboxConnecting?.();
      return null;
    }),
    dropboxDisconnect: command(() => {
      state.dropboxAccount = null;
      return null;
    }),
    dropboxListFolder: command(async (path: string) => {
      if (!state.dropboxAccount) throw new MockError("dropbox", "Dropbox no longer lets keefast in. Connect your account again.");
      await new Promise((resolve) => setTimeout(resolve, 250));
      const items = DROPBOX_TREE[path.toLowerCase()];
      if (!items) throw new MockError("dropbox", "That is no longer in Dropbox.");
      return items.map(([name, modified, size]) => {
        const isFolder = modified === undefined;
        return {
          name,
          path: `${path}/${name}`,
          isFolder,
          size: isFolder ? null : size!,
          modified: isFolder ? null : Date.parse(modified) / 1000,
          location: { provider: "dropbox" as const, path: `${path}/${name}` },
        };
      });
    }),
    approvePairing: command((id, name) => {
      const pending = state.pendingPairing;
      if (!pending || pending.id !== id) return null;
      requireName(name);
      const v = vault(pending.vault);
      if (v.pairings.includes(name.trim())) throw new MockError("invalid", "This Vault already has a browser with this name.");
      v.pairings.push(name.trim());
      state.pendingPairing = null;
      edited(v);
      return null;
    }),
    denyPairing: command((id) => {
      if (state.pendingPairing?.id === id) state.pendingPairing = null;
      changed();
      return null;
    }),
    answerAccess: command((id, allow, remember) => {
      const pending = state.pendingAccess;
      if (!pending || pending.id !== id) return null;
      state.pendingAccess = null;
      if (remember) {
        for (const [vaultId, entryId] of pending.entries) {
          const e = state.vaults.find((v) => v.id === vaultId)?.entries.find((x) => x.id === entryId);
          e?.access.push({ host: pending.site, allowed: allow });
        }
      }
      changed();
      return null;
    }),
    saveLogin: command((id, target: LoginTarget) => {
      const pending = state.pendingLogin;
      if (!pending || pending.id !== id) throw notFound();
      const v = vault(target.vault);
      if (target.kind === "update") {
        const e = v.entries.find((x) => x.id === target.entry);
        if (!e) throw notFound();
        keepHistory(e);
        Object.assign(e, { username: pending.username, password: pending.password });
      } else {
        let groupId = target.group;
        if (groupId === null) {
          groupId = v.browserGroup ?? `g${state.nextId++}`;
          if (!v.browserGroup) {
            v.groups.push({ id: groupId, name: "Browser", parent: v.groups.find((g) => g.parent === null)!.id });
            v.browserGroup = groupId;
          }
        }
        if (inBin(v, groupId)) throw new MockError("invalid", "Logins can't be saved in the Recycle Bin.");
        v.entries.push({
          id: `n${state.nextId++}`, title: pending.site, brand: "", username: pending.username, password: pending.password,
          url: `https://${pending.site}`, notes: "", tags: [], color: null, icon: { kind: "standard", number: 1 }, totp: null,
          attachments: [], fields: [], expires: null, created: now(), modified: now(), group: groupId, history: [], access: [],
        });
      }
      state.pendingLogin = null;
      edited(v);
      return null;
    }),
    cancelLogin: command((id) => {
      if (state.pendingLogin?.id === id) state.pendingLogin = null;
      changed();
      return null;
    }),
  };

  function parseTotp(setup: TotpSetup): MockTotp {
    const text = setup.secretOrUri.trim();
    if (!text) throw new MockError("invalid", "Enter the secret key or otpauth:// URI.");
    if (/^otpauth:\/\//i.test(text)) {
      let url: URL;
      try {
        url = new URL(text);
      } catch {
        throw new MockError("invalid", "This isn't an otpauth://totp/ URI.");
      }
      if (url.host.toLowerCase() !== "totp") throw new MockError("invalid", "This isn't an otpauth://totp/ URI.");
      const secret = url.searchParams.get("secret");
      if (!secret) throw new MockError("invalid", "Enter the secret key or otpauth:// URI.");
      const algorithm = (url.searchParams.get("algorithm") ?? "sha1").toLowerCase().replace("-", "");
      return {
        secret,
        algorithm: algorithm === "sha256" || algorithm === "sha512" ? algorithm : "sha1",
        digits: Number(url.searchParams.get("digits") ?? 6),
        period: Number(url.searchParams.get("period") ?? 30),
      };
    }
    if (!/^[A-Za-z2-7\s-]+=*$/.test(text)) {
      throw new MockError("invalid", "The secret key isn't valid: use the letters A–Z and digits 2–7.");
    }
    const digits = setup.digits.trim() ? Number(setup.digits) : 6;
    const period = setup.period.trim() ? Number(setup.period) : 30;
    if (!Number.isInteger(digits) || digits < 1 || digits > 10) throw new MockError("invalid", "A code has 1 to 10 digits.");
    if (!Number.isInteger(period) || period < 1 || period > 86400) throw new MockError("invalid", "The period is 1 to 86400 seconds.");
    return { secret: text.replace(/[\s-]/g, "").toUpperCase(), algorithm: setup.algorithm, digits, period };
  }

  const events = Object.fromEntries(
    (["workspaceChanged", "mergeNotice", "browserRequest", "clipboardCleared", "saveFailed"] as const).map((name) => [
      name,
      {
        listen: async (callback: (event: { payload: never }) => void) => {
          const set = listeners.get(name) ?? new Set();
          set.add(callback);
          listeners.set(name, set);
          return () => set.delete(callback);
        },
      },
    ]),
  ) as unknown as Events;

  function simulate(scene: MockScene) {
    if ((PLUGIN_SCENES as readonly string[]).includes(scene)) {
      plugins.simulate(scene as PluginScene);
      changed();
      return;
    }
    const open = readable();
    const first = open[0];
    switch (scene) {
      case "pairing":
        if (!first) return;
        state.pendingPairing = { id: state.nextId++, vault: first.id };
        emit("browserRequest", { kind: "pairing" });
        break;
      case "access": {
        state.pendingAccess = { id: state.nextId++, site: "github.com", entries: entriesFor("github.com") };
        emit("browserRequest", { kind: "access" });
        break;
      }
      case "login":
        // The prototype's story: a new password for Linear, which Work has an Entry for.
        state.pendingLogin = {
          id: state.nextId++,
          site: "linear.app",
          username: "arthur@rezende.dev",
          password: "L1near!Issues-2026",
          entries: entriesFor("linear.app", "arthur@rezende.dev"),
        };
        emit("browserRequest", { kind: "login" });
        break;
      case "lockUnsaved":
        if (!first) return;
        first.unsaved = true;
        state.pendingLock = { vault: first.id, canCancel: true };
        break;
      case "merge":
        if (!first) return;
        emit("mergeNotice", {
          vault: first.id,
          vaultName: first.name,
          message: `Merged changes made elsewhere into ${first.name}: 1 Entry updated, 2 Entries added`,
        });
        break;
      case "saveFailed":
        if (!first) return;
        first.saveFailed = true;
        emit("saveFailed", {
          vault: first.id,
          vaultName: first.name,
          message: "The Vault could not be saved.",
          detail: "could not access the Vault file: Permission denied (os error 13)",
        });
        break;
      case "unlocked-none":
        for (const v of state.vaults) v.locked = true;
        state.view = { scope: { kind: "allVaults" }, entry: null };
        break;
      case "dropbox-disconnected":
        state.dropboxAccount = null;
        break;
      case "sync-offline":
      case "sync-error":
        for (const v of state.vaults) if (v.sync) v.sync = scene === "sync-offline" ? "offline" : "error";
        break;
      case "browserError":
        state.browserHosts = "failed";
        break;
      case "browserNone":
        state.browserHosts = "none";
        break;
      case "browserServed":
        state.browserHosts = "served";
        break;
      case "configError":
        state.configError = "could not write ~/.config/keefast/config.json: Permission denied (os error 13)";
        break;
      case "closed": {
        // The prototype's Open a Vault story: Family locked, the others recent Vault files.
        const closed = state.vaults.filter((v) => !v.locked);
        state.vaults = state.vaults.filter((v) => v.locked);
        state.recents = [...closed.map((v) => v.location), ...state.recents];
        state.view = { scope: { kind: "allVaults" }, entry: null };
        break;
      }
    }
    changed();
  }

  const backend = {
    kind: "mock" as const,
    commands,
    events,
    simulate,
    reset() {
      state = initialState();
      plugins = createMockPluginStore(changed);
      changed();
    },
  };
  if (typeof window !== "undefined") {
    (window as unknown as { __keefastMock: typeof backend }).__keefastMock = backend;
    const scenes = new URLSearchParams(window.location.search).get("mock");
    for (const scene of scenes?.split(",").filter(Boolean) ?? []) {
      // A scene that only sends an event waits for the UI to listen; the others set the state
      // the UI first reads.
      if (scene === "merge" || scene === "saveFailed") setTimeout(() => simulate(scene), 1000);
      else simulate(scene as MockScene);
    }
  }
  return backend;
}


