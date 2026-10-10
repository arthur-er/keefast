// A Plugin's facet as a section of Entry editor A, such as the Card (card prototype variant A):
// drawn with the kit from what the manifest declares. What the user types goes through the
// Plugin's `format` hook (a card number in groups, a slash in MM/YY), and its `validate` hook's
// warnings show under the field; only an error keeps the Entry from saving. Protected Owned
// fields are revealed when focused, as protected custom fields are.

import { useEffect, useRef, useState } from "react";
import { CircleAlert, Eye, EyeOff, Trash2, TriangleAlert } from "lucide-react";

import { api } from "@/api";
import type { ProblemView } from "@/bindings";
import { Button, IconButton } from "@/kit";
import { cn } from "@/lib/utils";

import { backspaceFrom, caretAfterFormat, type FacetDraft, type OwnedRow } from "./editing";
import { facetIcon } from "./FacetSection";

/** What the Plugins say of the facets' values, by row key: each problem with the value it is about. */
export type FacetChecks = ReadonlyMap<number, { value: string; problem: ProblemView | null }>;

/**
 * Runs `validate` on every Owned field's value the editor has, each time it changes: typed,
 * formatted, or revealed. Values not revealed aren't checked: they stay in the backend.
 */
export function useFacetChecks(facets: readonly FacetDraft[]): FacetChecks {
  const [checks, setChecks] = useState<FacetChecks>(new Map());
  const asked = useRef(new Map<number, string>());
  useEffect(() => {
    for (const facet of facets) {
      for (const row of facet.rows) {
        if (row.value === null || asked.current.get(row.key) === row.value) continue;
        const value = row.value;
        asked.current.set(row.key, value);
        api
          .validateOwnedField(facet.plugin, facet.facet, row.field, value)
          .catch(() => null)
          .then((problem) =>
            setChecks((current) => (asked.current.get(row.key) === value ? new Map(current).set(row.key, { value, problem }) : current)),
          );
      }
    }
  }, [facets]);
  return checks;
}

/** The problem with a row's value as it is now, if the Plugin found one. */
export function problemOf(checks: FacetChecks, row: OwnedRow): ProblemView | null {
  const check = checks.get(row.key);
  return check && check.value === row.value ? check.problem : null;
}

/** The rows whose values the Plugins say can't be kept: the Entry doesn't save while there are any. */
export function rowsWithErrors(facets: readonly FacetDraft[], checks: FacetChecks): OwnedRow[] {
  return facets.flatMap((facet) => facet.rows.filter((row) => problemOf(checks, row)?.severity === "error"));
}

export function FacetEditor({
  facet,
  checks,
  shown,
  onShow,
  onRow,
  onFormatted,
  onReveal,
  onRemove,
}: {
  facet: FacetDraft;
  checks: FacetChecks;
  /** Keys of the protected rows shown as text. */
  shown: ReadonlySet<number>;
  onShow: (row: OwnedRow, show: boolean) => void;
  onRow: (key: number, change: Partial<OwnedRow>) => void;
  /** Replaces a row's value by `formatted`, if it is still `typed`. */
  onFormatted: (key: number, typed: string, formatted: string) => void;
  onReveal: (row: OwnedRow) => Promise<void>;
  onRemove: () => void;
}) {
  const Icon = facetIcon(facet.icon);

  const type = async (row: OwnedRow, input: HTMLInputElement) => {
    const typed = input.value;
    const caret = input.selectionStart ?? typed.length;
    onRow(row.key, { value: typed });
    let formatted: string;
    try {
      formatted = await api.formatOwnedField(facet.plugin, facet.facet, row.field, typed);
    } catch {
      // A Plugin that fails leaves the value as typed.
      return;
    }
    if (formatted === typed) return;
    onFormatted(row.key, typed, formatted);
    requestAnimationFrame(() => {
      if (document.activeElement !== input || input.value !== formatted) return;
      const at = caretAfterFormat(typed, caret, formatted);
      input.setSelectionRange(at, at);
    });
  };

  return (
    <div className="ee-sec" aria-label={facet.title}>
      <div className="ee-sec-head">
        <Icon className="ico-sm text-fg-2" />
        <h3>{facet.title}</h3>
        <Button variant="ghost" size="sm" onClick={onRemove}>
          <Trash2 /> {facet.editor.remove}
        </Button>
      </div>
      {facet.rows.map((row) => {
        const visible = !row.protected || shown.has(row.key);
        const problem = problemOf(checks, row);
        const formatted = row.kind !== "text";
        return (
          <div key={row.key} className="ee-kv top">
            <span className="k">{row.label}</span>
            <div className="vstack min-w-0 gap-1">
              <div className={cn("gi-group", row.kind === "monthYear" && "w-28", problem?.severity === "error" && "invalid")}>
                <input
                  aria-label={row.label}
                  data-owned-field={row.key}
                  className={cn(formatted && "mono", row.value === null && "kept")}
                  type={visible ? "text" : "password"}
                  inputMode={formatted ? "numeric" : undefined}
                  value={row.value ?? ""}
                  placeholder={row.value === null ? "••••••••" : (row.placeholder ?? "")}
                  spellCheck={false}
                  autoComplete="off"
                  aria-invalid={problem?.severity === "error" || undefined}
                  onFocus={() => void onReveal(row)}
                  onKeyDown={(event) => {
                    // Backspace after a space or slash `format` added deletes the digit before it.
                    const input = event.currentTarget;
                    if (!formatted || event.key !== "Backspace" || input.selectionStart !== input.selectionEnd) return;
                    const from = backspaceFrom(input.value, input.selectionStart ?? 0);
                    if (from !== input.selectionStart) input.setSelectionRange(from, from);
                  }}
                  onChange={(event) => void type(row, event.currentTarget)}
                />
                {row.protected && (
                  <IconButton
                    size="sm"
                    label={visible ? `Hide ${row.label}` : `Show ${row.label}`}
                    onClick={async () => {
                      if (!visible) await onReveal(row);
                      onShow(row, !visible);
                    }}
                  >
                    {visible ? <EyeOff /> : <Eye />}
                  </IconButton>
                )}
              </div>
              {problem && (
                <div className={cn("ee-hint", problem.severity === "error" && "is-error")} role="status">
                  {problem.severity === "error" ? <CircleAlert /> : <TriangleAlert />}
                  <span>{problem.message}</span>
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
