// The app shell (Vault browser C): the sidebar, the toolbar and Merge notice, and the main area,
// which shows the Entry list and detail, a locked Vault's card, or a page (settings).

import { type CSSProperties, useEffect, useRef, useState } from "react";
import { ArrowLeft, Settings } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";

import { api } from "@/api";
import type { AppSettings, PaneWidthsView } from "@/bindings";
import { keys, useSettings, useWorkspace } from "@/api/queries";
import { Button } from "@/kit";
import { cn } from "@/lib/utils";
import { EntryHistoryPage } from "@/screens/entry-history/EntryHistory";
import { DetailArea } from "@/screens/vault-browser/DetailArea";
import { EntryList } from "@/screens/vault-browser/EntryList";
import { LockedVault } from "@/screens/vault-browser/LockedVault";
import { VaultKeyCommand } from "@/screens/vault-key/ChangeVaultKey";

import { scopeVault } from "./hooks";
import { Sidebar } from "./Sidebar";
import { useShell } from "./state";
import { MergeNotice, Toolbar } from "./Toolbar";
import { useView } from "./view";

/** The widths Vault browser C draws, which a double-click on an edge goes back to. */
export const DEFAULT_WIDTHS: PaneWidthsView = { sidebar: 232, entryList: 360 };
const LIMITS = { sidebar: [180, 360], entryList: [260, 560] } as const;

export function Shell() {
  const widths = usePaneWidths();
  const { state } = useShell();
  return (
    <div className="vb" style={{ "--sidebar-w": `${widths.current.sidebar}px`, "--list-w": `${widths.current.entryList}px` } as CSSProperties}>
      <Sidebar />
      <Edge pane="sidebar" widths={widths} left={0} />
      <div className="vb-main">
        {state.page.kind === "browser" && state.detail === "history" ? (
          // Entry history B takes the Entry list's place too: versions beside the comparison.
          <EntryHistoryPage />
        ) : (
          <>
            <Toolbar />
            <MergeNotice />
            <div className="vb-panes">{state.page.kind === "browser" ? <Browser widths={widths} /> : <PagePlaceholder />}</div>
          </>
        )}
      </div>
      {/* Until Vault settings C (Tauri UI 6/7) embeds the change-key card. */}
      <VaultKeyCommand />
    </div>
  );
}

function Browser({ widths }: { widths: PaneWidths }) {
  const view = useView();
  const snapshot = useWorkspace().data;
  const vaultId = view ? scopeVault(view.scope) : null;
  const vault = snapshot?.vaults.find((v) => v.id === vaultId);
  if (vault && vault.state !== "unlocked") return <LockedVault key={vault.id} vault={vault} />;
  return (
    <>
      <EntryList />
      <Edge pane="entryList" widths={widths} left={widths.current.entryList} />
      <DetailArea />
    </>
  );
}

/** Settings and Vault settings arrive with Tauri UI 6/7; until then their page says so. */
function PagePlaceholder() {
  const { state, actions } = useShell();
  const title = state.page.kind === "settings" ? "Settings" : "Vault Settings";
  return (
    <div className="vb-empty flex-1">
      <Settings />
      <p>{title}</p>
      <span className="subtle text-sm">This page arrives with the settings screens.</span>
      <Button size="sm" onClick={actions.showBrowser} shortcut="Esc">
        <ArrowLeft /> Back to the Entries
      </Button>
    </div>
  );
}

interface PaneWidths {
  current: PaneWidthsView;
  set(widths: PaneWidthsView): void;
  /** Remembers `widths`, or the ones dragged to. */
  save(widths?: PaneWidthsView): void;
}

/** The panes' widths: the saved ones, or the ones being dragged. */
function usePaneWidths(): PaneWidths {
  const client = useQueryClient();
  const saved = useSettings().data?.paneWidths;
  const [dragged, setDragged] = useState<PaneWidthsView | null>(null);
  const current = dragged ?? saved ?? DEFAULT_WIDTHS;
  const latest = useRef(current);
  latest.current = current;
  return {
    current,
    set: setDragged,
    save: (widths = latest.current) => {
      // The backend doesn't announce new widths (nothing else changes), so the cache learns them here.
      client.setQueryData<AppSettings>(keys.settings, (settings) => settings && { ...settings, paneWidths: widths });
      setDragged(null);
      void api.setPaneWidths(widths);
    },
  };
}

function Edge({ pane, widths, left }: { pane: keyof PaneWidthsView; widths: PaneWidths; left: number }) {
  const [dragging, setDragging] = useState(false);
  const start = useRef<{ x: number; width: number } | null>(null);
  useEffect(() => {
    if (!dragging) return;
    const move = (event: PointerEvent) => {
      if (!start.current) return;
      const [min, max] = LIMITS[pane];
      const width = Math.round(Math.min(max, Math.max(min, start.current.width + event.clientX - start.current.x)));
      widths.set({ ...widths.current, [pane]: width });
    };
    const up = () => {
      setDragging(false);
      start.current = null;
      widths.save();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  });
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={pane === "sidebar" ? "Resize the sidebar" : "Resize the Entry list"}
      className={cn("vb-edge", dragging && "is-dragging")}
      style={{ left: pane === "sidebar" ? `var(--sidebar-w)` : left }}
      onPointerDown={(event) => {
        event.preventDefault();
        start.current = { x: event.clientX, width: widths.current[pane] };
        setDragging(true);
      }}
      onDoubleClick={() => {
        const reset = { ...widths.current, [pane]: DEFAULT_WIDTHS[pane] };
        widths.set(reset);
        widths.save(reset);
      }}
    />
  );
}
