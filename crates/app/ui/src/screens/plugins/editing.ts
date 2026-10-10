// Plugins' facets in Entry editor A, as plain data: a facet's section in the draft, adding and
// removing it, and where the caret goes when the Plugin's `format` hook rewrites what the user
// typed. Nothing here knows any one Plugin: a facet's section is drawn from what its manifest
// declares (ADR 0009).

import type { FacetEditorView, FacetTemplate, FacetView, OwnedFieldKind } from "@/bindings";

/** One Owned field in the editor, edited as the custom field it is. */
export interface OwnedRow {
  /** Stable while editing, for React. */
  key: number;
  /** How its Plugin names it, such as `number`. */
  field: string;
  /** The custom field's name, such as `Card number`. */
  name: string;
  label: string;
  kind: OwnedFieldKind;
  placeholder: string | null;
  /** `null`: the saved value, not revealed. */
  value: string | null;
  protected: boolean;
  /** The field's name in the saved Entry, for a field it had; `null` for a new one. */
  original: string | null;
  /** The saved value, once revealed: a value equal to it saves as kept. */
  savedValue: string | null;
}

/** A facet's section in the draft, such as the Card. */
export interface FacetDraft {
  plugin: string;
  facet: string;
  title: string;
  icon: string | null;
  editor: FacetEditorView;
  /** The names of the fields an Entry has the facet by: they are saved even while empty, so the
   * facet stays on the Entry. */
  detect: string[];
  rows: OwnedRow[];
}

/** The custom fields a draft edits outside facets, as `FieldRow` in draft.ts has them. */
interface CustomRow {
  name: string;
  value: string | null;
  protected: boolean;
  original: string | null;
  savedValue: string | null;
}

export const sameFacet = (a: { plugin: string; facet: string }, b: { plugin: string; facet: string }) =>
  a.plugin === b.plugin && a.facet === b.facet;

/** The sections of the facets an Entry has that the editor edits, and the indexes of the custom
 * fields they own, which the custom fields list leaves out. */
export function facetDrafts(facets: readonly FacetView[], rowKey: () => number): { drafts: FacetDraft[]; owned: Set<number> } {
  const owned = new Set<number>();
  const drafts = facets.flatMap((facet): FacetDraft[] => {
    if (!facet.editor) return [];
    return [
      {
        plugin: facet.plugin,
        facet: facet.facet,
        title: facet.title,
        icon: facet.icon,
        editor: facet.editor,
        detect: facet.detect,
        rows: facet.fields.map((field) => {
          if (field.index !== null) owned.add(field.index);
          const saved = field.index === null ? "" : (field.value ?? (field.isEmpty ? "" : null));
          return {
            key: rowKey(),
            field: field.key,
            name: field.name,
            label: field.label,
            kind: field.kind,
            placeholder: field.placeholder,
            value: saved,
            protected: field.protected,
            original: field.index === null ? null : field.name,
            savedValue: field.index === null ? null : saved,
          };
        }),
      },
    ];
  });
  return { drafts, owned };
}

/** A facet added to the draft: its Owned fields, empty, except those the draft has as custom
 * fields already, which move into the section with their values. */
export function addFacet<Row extends CustomRow>(
  template: FacetTemplate,
  fields: readonly Row[],
  rowKey: () => number,
): { facet: FacetDraft; fields: Row[] } {
  const names = new Set(template.fields.map((field) => field.name));
  const facet: FacetDraft = {
    plugin: template.plugin,
    facet: template.facet,
    title: template.title,
    icon: template.icon,
    editor: template.editor!,
    detect: template.detect,
    rows: template.fields.map((field) => {
      const had = fields.find((row) => row.name.trim() === field.name);
      return {
        key: rowKey(),
        field: field.key,
        name: field.name,
        label: field.label,
        kind: field.kind,
        placeholder: field.placeholder,
        value: had ? had.value : "",
        protected: had ? had.protected : field.protected,
        original: had ? had.original : null,
        savedValue: had ? had.savedValue : null,
      };
    }),
  };
  return { facet, fields: fields.filter((row) => !names.has(row.name.trim())) };
}

/** The facets the editor can add to a draft that hasn't got them, in the Plugins' order. */
export function addableFacets(templates: readonly FacetTemplate[], drafts: readonly FacetDraft[]): FacetTemplate[] {
  return templates.filter((template) => template.editor !== null && !drafts.some((draft) => sameFacet(draft, template)));
}

/** Whether a row saves: one the Entry had, one with a value, or one the facet is detected by. */
export function rowSaves(facet: FacetDraft, row: OwnedRow): boolean {
  return row.original !== null || (row.value ?? "") !== "" || facet.detect.includes(row.name);
}

/** Letters and digits: what a `format` hook keeps, unlike the spaces and slashes it adds. */
const significant = (char: string) => /[\p{L}\p{N}]/u.test(char);

/**
 * Where the caret goes when `typed`, with the caret at `caret`, is replaced by `formatted`: after
 * as many letters and digits as were before it, so typing or deleting mid-string keeps the caret
 * where the user is. A caret at the end stays at the end, past an added separator ("08/").
 */
export function caretAfterFormat(typed: string, caret: number, formatted: string): number {
  if (caret >= typed.length) return formatted.length;
  let wanted = [...typed.slice(0, caret)].filter(significant).length;
  if (wanted === 0) return 0;
  for (let at = 0; at < formatted.length; at++) {
    if (significant(formatted[at]!) && --wanted === 0) return at + 1;
  }
  return formatted.length;
}

/** Where Backspace at `caret` should delete from: past a separator, onto the character before
 * it, so deleting over "4111 |1111" removes the 1 rather than a space `format` puts back. */
export function backspaceFrom(value: string, caret: number): number {
  let at = caret;
  while (at > 0 && !significant(value[at - 1]!)) at--;
  return at > 0 ? at : caret;
}
