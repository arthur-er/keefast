// A Plugin's facet on an Entry, such as its Card, as a section of the Entry detail's Overview
// (card prototype variant A). Drawn with the kit from what the manifest declares and the labels
// the Plugin derived: no Plugin code runs here (ADR 0009). Protected values show as the Plugin's
// label or masked, and are revealed and copied as any custom field's, by index.

import { useEffect, useState } from "react";
import { CreditCard, Eye, EyeOff, Landmark, type LucideIcon, Puzzle } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { EntryDetail, FacetView, OwnedFieldView } from "@/bindings";
import { Badge, toast } from "@/kit";
import { cn } from "@/lib/utils";
import { type CopyHandler, FieldRow } from "@/screens/vault-browser/FieldRow";

import { monthYearState } from "./fields";

/** The icons a facet may name in its manifest. */
const ICONS: Record<string, LucideIcon> = { "credit-card": CreditCard, landmark: Landmark };

/** A facet's icon, by the name its manifest gives; a generic one for a name the kit lacks. */
export const facetIcon = (name: string | null): LucideIcon => (name && ICONS[name]) || Puzzle;

export function FacetSection({
  entry,
  facet,
  copied,
  onCopy,
}: {
  entry: EntryDetail;
  facet: FacetView;
  copied: string | null;
  onCopy: CopyHandler;
}) {
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  // Revealed values go when the window is left, or another Entry is shown.
  useEffect(() => {
    setRevealed({});
    const hide = () => setRevealed({});
    window.addEventListener("blur", hide);
    return () => window.removeEventListener("blur", hide);
  }, [entry.vault, entry.id]);

  const toggle = async (field: OwnedFieldView) => {
    if (field.key in revealed) {
      const next = { ...revealed };
      delete next[field.key];
      return setRevealed(next);
    }
    if (field.index === null) return;
    try {
      const value = await api.revealCustomField(entry.vault, entry.id, field.index);
      setRevealed((current) => ({ ...current, [field.key]: value }));
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };

  const Icon = facetIcon(facet.icon);
  return (
    <div className="vb-sect mt-0 mb-6" aria-label={facet.title}>
      <div className="vb-sect-head">
        <Icon className="ico-sm" /> {facet.title}
      </div>
      <div className="vb-fields">
        {facet.fields.map((field) => {
          const key = `facet:${facet.plugin}:${facet.facet}:${field.key}`;
          const present = field.index !== null && !field.isEmpty;
          const shown = revealed[field.key];
          return (
            <FieldRow
              key={field.key}
              label={field.label}
              copied={copied === key}
              onCopy={present && facet.canCopy ? () => void onCopy({ kind: "customField", index: field.index! }, field.name, key) : undefined}
              actions={
                present &&
                field.protected && (
                  <button
                    type="button"
                    className="btn ghost sm icon"
                    aria-label={`${shown === undefined ? "Show" : "Hide"} ${field.label}`}
                    title={shown === undefined ? "Show" : "Hide"}
                    onClick={() => void toggle(field)}
                  >
                    {shown === undefined ? <Eye /> : <EyeOff />}
                  </button>
                )
              }
            >
              <FieldValue field={field} present={present} revealed={shown} />
            </FieldRow>
          );
        })}
      </div>
    </div>
  );
}

function FieldValue({ field, present, revealed }: { field: OwnedFieldView; present: boolean; revealed: string | undefined }) {
  if (!present) return <span className="subtle">None</span>;
  const mono = field.kind !== "text";
  if (revealed !== undefined) return <span className={cn("val selectable", mono && "mono")}>{revealed}</span>;
  // The Plugin's text stands in for the value, protected or not.
  if (field.text !== null) return <span className={cn("val", mono && "mono")}>{field.text}</span>;
  if (field.value === null) return <span className="val vb-pw masked">••••</span>;
  return (
    <>
      <span className={cn("val selectable", mono && "mono")}>{field.value}</span>
      {field.kind === "monthYear" && <MonthYearBadge value={field.value} />}
    </>
  );
}

function MonthYearBadge({ value }: { value: string }) {
  const state = monthYearState(value);
  if (state === "expired") return <Badge tone="danger">Expired</Badge>;
  if (state === "soon") return <Badge tone="warn">Expires soon</Badge>;
  if (state === "invalid") return <Badge tone="danger">Not MM/YY</Badge>;
  return null;
}
