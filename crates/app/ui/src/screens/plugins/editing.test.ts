import { describe, expect, it } from "vitest";

import type { FacetTemplate, FacetView } from "@/bindings";

import { addableFacets, addFacet, backspaceFrom, caretAfterFormat, facetDrafts, rowSaves } from "./editing";

const owned = [
  { key: "number", name: "Card number", label: "Number", kind: "digits", protected: true, placeholder: "1234 5678 9012 3456" },
  { key: "holder", name: "Cardholder", label: "Cardholder", kind: "text", protected: false, placeholder: null },
  { key: "expiry", name: "Expiry", label: "Expiry", kind: "monthYear", protected: false, placeholder: "MM/YY" },
  { key: "cvv", name: "CVV", label: "CVV", kind: "digits", protected: true, placeholder: null },
] as const;

const template: FacetTemplate = {
  plugin: "keefast.card",
  facet: "card",
  title: "Card",
  icon: "credit-card",
  detect: ["Card number"],
  editor: { add: "Add card", remove: "Remove card" },
  fields: owned.map((field) => ({ ...field })),
};

/** The Card as `get_entry` finds it: the number at index 1, the expiry at 3, no CVV. */
const view: FacetView = {
  commands: [],
  plugin: "keefast.card",
  facet: "card",
  title: "Card",
  icon: "credit-card",
  label: "Visa •••• 1111",
  canCopy: true,
  detect: ["Card number"],
  editor: { add: "Add card", remove: "Remove card" },
  fields: owned.map((field) => ({
    key: field.key,
    name: field.name,
    label: field.label,
    kind: field.kind,
    placeholder: field.placeholder,
    index: field.key === "number" ? 1 : field.key === "expiry" ? 3 : field.key === "holder" ? 2 : null,
    protected: field.protected,
    value: field.key === "expiry" ? "08/31" : field.key === "holder" ? "" : null,
    isEmpty: field.key === "holder",
    text: field.key === "number" ? "Visa •••• 1111" : null,
  })),
};

let next = 1;
const rowKey = () => next++;

describe("facetDrafts", () => {
  it("keeps protected values in the backend and says which custom fields the sections own", () => {
    const { drafts, owned: indexes } = facetDrafts([view], rowKey);
    expect([...indexes].sort()).toEqual([1, 2, 3]);
    expect(drafts[0]!.rows.map((row) => [row.name, row.value, row.original])).toEqual([
      ["Card number", null, "Card number"],
      ["Cardholder", "", "Cardholder"],
      ["Expiry", "08/31", "Expiry"],
      ["CVV", "", null],
    ]);
  });

  it("leaves a facet without an editor section to the custom fields", () => {
    const { drafts, owned: indexes } = facetDrafts([{ ...view, editor: null }], rowKey);
    expect(drafts).toEqual([]);
    expect(indexes.size).toBe(0);
  });
});

describe("addFacet", () => {
  it("adds empty Owned fields, taking the ones the Entry has as custom fields with their values", () => {
    const fields = [
      { key: 1, name: "Account", value: "42", protected: false, original: "Account", savedValue: "42" },
      { key: 2, name: "CVV", value: null, protected: true, original: "CVV", savedValue: null },
    ];
    const added = addFacet(template, fields, rowKey);
    expect(added.fields.map((row) => row.name)).toEqual(["Account"]);
    expect(added.facet.rows.map((row) => [row.name, row.value, row.protected, row.original])).toEqual([
      ["Card number", "", true, null],
      ["Cardholder", "", false, null],
      ["Expiry", "", false, null],
      ["CVV", null, true, "CVV"],
    ]);
  });

  it("offers only facets with a section that the draft hasn't got", () => {
    const { drafts } = facetDrafts([view], rowKey);
    expect(addableFacets([template], drafts)).toEqual([]);
    expect(addableFacets([template], [])).toEqual([template]);
    expect(addableFacets([{ ...template, editor: null }], [])).toEqual([]);
  });

  it("saves the fields a facet is detected by even while empty, and new others once filled", () => {
    const { facet } = addFacet(template, [], rowKey);
    expect(facet.rows.filter((row) => rowSaves(facet, row)).map((row) => row.name)).toEqual(["Card number"]);
  });
});

describe("caretAfterFormat", () => {
  it("stays at the end, past a separator the Plugin added", () => {
    expect(caretAfterFormat("083", 3, "08/3")).toBe(4);
    expect(caretAfterFormat("8", 1, "08/")).toBe(3);
    expect(caretAfterFormat("41111", 5, "4111 1")).toBe(6);
  });

  it("stays after the same digit when typing or deleting mid-string", () => {
    // A 2 typed after "4111 1": the groups move, the caret stays after the 2.
    expect(caretAfterFormat("4111 12111", 7, "4111 1211 1")).toBe(7);
    // A digit deleted from the first group.
    expect(caretAfterFormat("411 1111 1", 2, "4111 1111")).toBe(2);
    expect(caretAfterFormat("4111 1111", 0, "4111 1111")).toBe(0);
  });
});

describe("backspaceFrom", () => {
  it("steps over separators onto the digit before", () => {
    expect(backspaceFrom("4111 1111", 5)).toBe(4);
    expect(backspaceFrom("08/", 3)).toBe(2);
    expect(backspaceFrom("4111 1111", 3)).toBe(3);
    expect(backspaceFrom(" ", 1)).toBe(1);
  });
});
