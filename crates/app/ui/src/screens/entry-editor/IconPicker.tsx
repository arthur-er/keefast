// The Entry's icon, with a pencil: a popover of KeePass's standard icons (filtered by name) and
// the Vault's custom icons, where a PNG can be imported as a new one.

import { useState } from "react";
import { ImageUp, Key, Pencil, Search } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { IconChoice, VaultId } from "@/bindings";
import { useCustomIcons } from "@/api/queries";
import { Button, Kbd, Popover, PopoverContent, PopoverTrigger, STANDARD_ICONS, toast } from "@/kit";
import { cn } from "@/lib/utils";

import { filterStandardIcons, STANDARD_ICON_NAMES } from "./icons";

/** An icon choice drawn as a tile of `size` pixels. */
export function IconTile({ icon, vault, size, faviconUrl = null }: { icon: IconChoice; vault: VaultId; size: number; faviconUrl?: string | null }) {
  const custom = useCustomIcons(icon.kind === "custom" ? vault : undefined).data;
  const radius = size >= 40 ? 12 : size >= 24 ? 7 : 5;
  if (faviconUrl || icon.kind === "custom") {
    const url = faviconUrl ?? (icon.kind === "custom" ? custom?.find((candidate) => candidate.id === icon.id)?.url : undefined);
    return (
      <span className="vb-avatar is-image" style={{ width: size, height: size, borderRadius: radius }}>
        {url ? <img src={url} alt="" /> : null}
      </span>
    );
  }
  const Icon = STANDARD_ICONS[icon.number] ?? Key;
  return (
    <span className="ee-std-tile" style={{ width: size, height: size, borderRadius: radius }}>
      <Icon style={{ width: Math.round(size * 0.5), height: Math.round(size * 0.5) }} />
    </span>
  );
}

export function IconPicker({
  icon,
  vault,
  faviconUrl,
  onChange,
}: {
  icon: IconChoice;
  vault: VaultId;
  faviconUrl: string | null;
  onChange: (icon: IconChoice) => void;
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const custom = useCustomIcons(vault).data ?? [];
  const shown = filterStandardIcons(filter);
  const pick = (choice: IconChoice) => {
    onChange(choice);
    setOpen(false);
  };
  const importIcon = async () => {
    try {
      const id = await api.importCustomIcon(vault);
      if (id) {
        pick({ kind: "custom", id });
        toast("Imported the image into this Vault's icons", { icon: ImageUp });
      }
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setFilter("");
      }}
    >
      <PopoverTrigger asChild>
        <button type="button" className="ee-icon-btn" aria-label="Icon · Change…" title="Icon · Change…">
          <IconTile icon={icon} vault={vault} size={40} faviconUrl={faviconUrl} />
          <span className="pip">
            <Pencil />
          </span>
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-[332px]" aria-label="Icon">
        <div className="ee-pop-head">
          <b>Icon</b>
          <span className="spacer" />
          <Kbd combo="Escape" />
        </div>
        <label className="ee-pop-search">
          <Search className="ico-sm" />
          <input value={filter} placeholder="Filter standard icons" aria-label="Filter standard icons" onChange={(event) => setFilter(event.target.value)} />
        </label>
        <div className="ee-pop-sec">
          <div className="menu-label">Standard icons</div>
          <div className="ee-icon-grid" role="listbox" aria-label="Standard icons">
            {shown.length === 0 ? (
              <span className="subtle col-span-full px-0.5 py-1.5 text-sm">No standard icon matches.</span>
            ) : (
              shown.map((number) => {
                const Icon = STANDARD_ICONS[number] ?? Key;
                const on = icon.kind === "standard" && icon.number === number;
                return (
                  <button
                    key={number}
                    type="button"
                    role="option"
                    aria-selected={on}
                    aria-label={STANDARD_ICON_NAMES[number]}
                    title={STANDARD_ICON_NAMES[number]}
                    className={cn("ee-icon-cell", on && "is-on")}
                    onClick={() => pick({ kind: "standard", number })}
                  >
                    <Icon />
                  </button>
                );
              })
            )}
          </div>
        </div>
        <div className="menu-sep m-0" />
        <div className="ee-pop-sec">
          <div className="menu-label">
            This Vault's icons
            <Button variant="ghost" size="sm" onClick={() => void importIcon()}>
              <ImageUp /> Import PNG…
            </Button>
          </div>
          {custom.length === 0 ? (
            <div className="subtle px-0.5 pb-1 text-sm">None yet.</div>
          ) : (
            <div className="ee-icon-grid" role="listbox" aria-label="This Vault's icons">
              {custom.map((candidate, index) => {
                const on = icon.kind === "custom" && icon.id === candidate.id;
                return (
                  <button
                    key={candidate.id}
                    type="button"
                    role="option"
                    aria-selected={on}
                    aria-label={`Custom icon ${index + 1}`}
                    className={cn("ee-icon-cell", on && "is-on")}
                    onClick={() => pick({ kind: "custom", id: candidate.id })}
                  >
                    <img src={candidate.url} alt="" />
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
