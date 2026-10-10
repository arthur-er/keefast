// What the shell shows besides what the backend keeps (`snapshot.view`: the scope and the
// selected Entry): the page in the main area, the detail area's mode, the overlay, Open a Vault,
// and the Entry list's search and sort.
//
// Where later screens plug in (DESIGN.md §7, "One shell"):
// - **Detail area.** `detail` is the mode of the selected Entry's pane: `view` (Vault browser C's
//   tabbed detail), `edit` (Entry editor A, edit in place: Tauri UI 3/7), `history` (Entry
//   history B: 4/7). `editEntry()`, `showHistory()` and `showEntry()` switch it; the pane
//   (`screens/vault-browser/DetailArea.tsx`) renders the screen for the mode.
// - **Pages.** `page` is what fills the main area right of the sidebar: the Vault browser's list
//   and detail, or General / Vault settings C (6/7), which replace them and keep the sidebar.
//   `openSettings(section)` and `openVaultSettings(vault)` go there; `showBrowser()` comes back.
// - **Overlays.** `overlay` floats over the shell, one at a time: the command palette A (4/7),
//   confirmations, an attachment's preview. Add a kind to `Overlay` and render it in
//   `shell/Overlays.tsx`. Browser requests and the lock prompt are separate (`PendingRequests`):
//   the backend asks for them. Popovers (generator, menus) anchor to their trigger instead.
// - **Open a Vault.** `openVault` replaces the whole window with Open a Vault B; `new` is its
//   New Vault state (5/7).

import { createContext, type ReactNode, useContext, useMemo, useReducer } from "react";

import type { MergeNotice, SortOrder, VaultId } from "@/bindings";

export type Page =
  | { kind: "browser" }
  | { kind: "settings"; section?: string }
  | { kind: "vaultSettings"; vault: VaultId; section?: string };

export type DetailMode = "view" | "edit" | "history";

export interface ConfirmOverlay {
  kind: "confirm";
  title: string;
  description: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
}

export interface AttachmentOverlay {
  kind: "attachment";
  vault: VaultId;
  entry: string;
  index: number;
  name: string;
}

export type Overlay = { kind: "palette" } | ConfirmOverlay | AttachmentOverlay;

export type OpenVaultMode = "unlock" | "new";

export interface ShellState {
  page: Page;
  detail: DetailMode;
  overlay: Overlay | null;
  /** Open a Vault B over the whole window, when the user asked for it. (With no Vault unlocked
   * it shows anyway.) */
  openVault: OpenVaultMode | null;
  search: string;
  sort: SortOrder;
  /** The last Merge notice, until dismissed. */
  merge: (MergeNotice & { at: number }) | null;
  /** The Vault the switcher shows while the list shows a tag, a colour or every Vault. */
  lastVault: VaultId | null;
  /** The Group being renamed in the sidebar, as `<vault>:<group>`. */
  renamingGroup: string | null;
}

type Action =
  | { type: "page"; page: Page }
  | { type: "detail"; mode: DetailMode }
  | { type: "overlay"; overlay: Overlay | null }
  | { type: "togglePalette" }
  | { type: "openVault"; mode: OpenVaultMode | null }
  | { type: "search"; search: string }
  | { type: "sort"; sort: SortOrder }
  | { type: "merge"; merge: ShellState["merge"] }
  | { type: "lastVault"; vault: VaultId }
  | { type: "renameGroup"; group: string | null };

const initial: ShellState = {
  page: { kind: "browser" },
  detail: "view",
  overlay: null,
  openVault: null,
  search: "",
  sort: "title",
  merge: null,
  lastVault: null,
  renamingGroup: null,
};

export function shellReducer(state: ShellState, action: Action): ShellState {
  switch (action.type) {
    case "page":
      return { ...state, page: action.page, detail: "view", openVault: null };
    case "detail":
      return { ...state, detail: action.mode, page: { kind: "browser" } };
    case "overlay":
      return { ...state, overlay: action.overlay };
    case "togglePalette":
      return { ...state, overlay: state.overlay?.kind === "palette" ? null : { kind: "palette" } };
    case "openVault":
      return { ...state, openVault: action.mode, overlay: null };
    case "search":
      return { ...state, search: action.search };
    case "sort":
      return { ...state, sort: action.sort };
    case "merge":
      return { ...state, merge: action.merge };
    case "lastVault":
      return state.lastVault === action.vault ? state : { ...state, lastVault: action.vault };
    case "renameGroup":
      return { ...state, renamingGroup: action.group };
  }
}

export interface ShellActions {
  showBrowser(): void;
  openSettings(section?: string): void;
  openVaultSettings(vault: VaultId, section?: string): void;
  /** Back to the selected Entry's detail from editing or its history. */
  showEntry(): void;
  editEntry(): void;
  showHistory(): void;
  openOverlay(overlay: Overlay): void;
  closeOverlay(): void;
  togglePalette(): void;
  confirm(options: Omit<ConfirmOverlay, "kind">): void;
  showOpenVault(mode?: OpenVaultMode): void;
  closeOpenVault(): void;
  setSearch(search: string): void;
  setSort(sort: SortOrder): void;
  showMerge(notice: MergeNotice): void;
  dismissMerge(): void;
  rememberVault(vault: VaultId): void;
  /** Starts renaming a Group in the sidebar (`<vault>:<group>`), or stops. */
  renameGroup(group: string | null): void;
}

const ShellContext = createContext<{ state: ShellState; actions: ShellActions } | null>(null);

export function ShellProvider({ children, initialState }: { children: ReactNode; initialState?: Partial<ShellState> }) {
  const [state, dispatch] = useReducer(shellReducer, { ...initial, ...initialState });
  const actions = useMemo<ShellActions>(
    () => ({
      showBrowser: () => dispatch({ type: "page", page: { kind: "browser" } }),
      openSettings: (section) => dispatch({ type: "page", page: { kind: "settings", section } }),
      openVaultSettings: (vault, section) => dispatch({ type: "page", page: { kind: "vaultSettings", vault, section } }),
      showEntry: () => dispatch({ type: "detail", mode: "view" }),
      editEntry: () => dispatch({ type: "detail", mode: "edit" }),
      showHistory: () => dispatch({ type: "detail", mode: "history" }),
      openOverlay: (overlay) => dispatch({ type: "overlay", overlay }),
      closeOverlay: () => dispatch({ type: "overlay", overlay: null }),
      togglePalette: () => dispatch({ type: "togglePalette" }),
      confirm: (options) => dispatch({ type: "overlay", overlay: { kind: "confirm", ...options } }),
      showOpenVault: (mode = "unlock") => dispatch({ type: "openVault", mode }),
      closeOpenVault: () => dispatch({ type: "openVault", mode: null }),
      setSearch: (search) => dispatch({ type: "search", search }),
      setSort: (sort) => dispatch({ type: "sort", sort }),
      showMerge: (notice) => dispatch({ type: "merge", merge: { ...notice, at: Date.now() } }),
      dismissMerge: () => dispatch({ type: "merge", merge: null }),
      rememberVault: (vault) => dispatch({ type: "lastVault", vault }),
      renameGroup: (group) => dispatch({ type: "renameGroup", group }),
    }),
    [],
  );
  const value = useMemo(() => ({ state, actions }), [state, actions]);
  return <ShellContext.Provider value={value}>{children}</ShellContext.Provider>;
}

export function useShell() {
  const shell = useContext(ShellContext);
  if (!shell) throw new Error("useShell outside ShellProvider");
  return shell;
}
