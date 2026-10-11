import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FacetTemplate } from "@/bindings";

// The backend, held in the test's hands: each reveal and each `format` answers when the test says,
// in any order, as `?latency=` makes the mock do.
const backend = vi.hoisted(() => ({
  reveals: [] as { index: number; answer: (value: string) => void }[],
  formats: [] as { value: string; answer: (formatted: string) => void }[],
}));

vi.mock("@/api", () => ({
  api: {
    revealCustomField: (_vault: number, _entry: string, index: number) =>
      new Promise<string>((answer) => backend.reveals.push({ index, answer })),
    formatOwnedField: (_plugin: string, _facet: string, _key: string, value: string) =>
      new Promise<string>((answer) => backend.formats.push({ value, answer })),
  },
  errorMessage: String,
}));

const { addFacet, facetDrafts, withChange, withFormatted, withRevealed } = await import("./editing");
const { FacetEditor } = await import("./FacetEditor");
const { api } = await import("@/api");

const template: FacetTemplate = {
  plugin: "keefast.card",
  facet: "card",
  title: "Card",
  icon: "credit-card",
  detect: ["Card number"],
  editor: { add: "Add card", remove: "Remove card" },
  fields: [
    { key: "number", name: "Card number", label: "Number", kind: "digits", protected: true, placeholder: null },
    { key: "cvv", name: "CVV", label: "CVV", kind: "digits", protected: true, placeholder: null },
  ],
};

let next = 1;
const rowKey = () => next++;

/** A Card as Entry editor A edits it: the same state changes, `withRevealed` on a reveal's answer. */
function Harness({ saved }: { saved: boolean }) {
  const [facets, setFacets] = useState(() =>
    saved
      ? facetDrafts(
          [
            {
              ...template,
              label: null,
              canCopy: true,
              commands: [],
              fields: template.fields.map((field, index) => ({
                ...field,
                index,
                value: null,
                isEmpty: false,
                text: null,
              })),
            },
          ],
          rowKey,
        ).drafts
      : [addFacet(template, [], rowKey).facet],
  );
  latest.facets = facets;
  const facet = facets[0]!;
  return (
    <FacetEditor
      facet={facet}
      checks={new Map()}
      shown={new Set(facet.rows.map((row) => row.key))}
      onShow={() => {}}
      onRow={(key, change) => setFacets((current) => withChange(current, key, change))}
      onFormatted={(key, typed, formatted) => setFacets((current) => withFormatted(current, key, typed, formatted))}
      onReveal={async (row) => {
        if (row.value !== null || row.original === null) return;
        const value = await api.revealCustomField(1, "e1", facet.rows.indexOf(row));
        setFacets((current) => withRevealed(current, row.key, value));
      }}
      onRemove={() => {}}
    />
  );
}
const latest = { facets: [] as ReturnType<typeof facetDrafts>["drafts"] };
const row = (label: string) => latest.facets[0]!.rows.find((candidate) => candidate.label === label)!;
afterEach(cleanup);

const input = (label: string) => screen.getByLabelText(label) as HTMLInputElement;

/** Types `value` into the field, as one keystroke leaves it. */
const typeInto = (label: string, value: string) =>
  act(() => {
    fireEvent.change(input(label), { target: { value } });
  });

describe("a protected Owned field revealed while the user types", () => {
  beforeEach(() => {
    backend.reveals = [];
    backend.formats = [];
  });

  it("keeps exactly what the user typed when the reveal answers after the first keystroke", async () => {
    render(<Harness saved />);
    act(() => input("CVV").focus());
    expect(backend.reveals).toHaveLength(1);
    await typeInto("CVV", "1");
    await typeInto("CVV", "12");
    // The saved CVV, 481, comes back now: too late, the user has typed over it.
    await act(async () => backend.reveals[0]!.answer("481"));
    for (const format of backend.formats) await act(async () => format.answer(format.value));
    expect(input("CVV").value).toBe("12");
    expect(row("CVV")).toMatchObject({ value: "12", savedValue: null });
  });

  it("lets the user edit the revealed value once it has come", async () => {
    render(<Harness saved />);
    act(() => input("CVV").focus());
    await act(async () => backend.reveals[0]!.answer("481"));
    expect(input("CVV").value).toBe("481");
    await typeInto("CVV", "4812");
    await act(async () => backend.formats[0]!.answer("4812"));
    expect(row("CVV")).toMatchObject({ value: "4812", savedValue: "481" });
  });

  it("never reveals an untouched field", () => {
    render(<Harness saved />);
    expect(backend.reveals).toEqual([]);
    expect(row("CVV").value).toBeNull();
  });
});

describe("format's answers coming back out of order", () => {
  beforeEach(() => {
    backend.reveals = [];
    backend.formats = [];
  });

  it("shows the answer to the last keystroke, never an older one", async () => {
    render(<Harness saved={false} />);
    const keystrokes = ["4", "41", "411", "4111", "41111", "411111"];
    for (const typed of keystrokes) await typeInto("Number", typed);
    expect(backend.formats.map((format) => format.value)).toEqual(keystrokes);
    const grouped = (value: string) => value.replace(/(\d{4})(?=\d)/g, "$1 ");

    // The last answer first, then every older one.
    const [last, ...older] = [...backend.formats].reverse();
    await act(async () => last!.answer(grouped(last!.value)));
    expect(input("Number").value).toBe("4111 11");
    for (const format of older) await act(async () => format.answer(grouped(format.value)));
    expect(input("Number").value).toBe("4111 11");
    expect(row("Number").value).toBe("4111 11");
  });

  it("drops an answer for a value the user has typed past, even when the newer one hasn't come", async () => {
    render(<Harness saved={false} />);
    await typeInto("Number", "41111");
    await typeInto("Number", "411112");
    await act(async () => backend.formats[0]!.answer("4111 1"));
    expect(row("Number").value).toBe("411112");
    await act(async () => backend.formats[1]!.answer("4111 12"));
    expect(row("Number").value).toBe("4111 12");
  });

  it("puts the caret back after the digit typed mid-string, however late the answer", async () => {
    render(<Harness saved={false} />);
    await typeInto("Number", "4111 1111");
    await act(async () => backend.formats[0]!.answer("4111 1111"));
    // A 9 typed after "4111 1", the caret after it.
    act(() => {
      input("Number").focus();
      fireEvent.change(input("Number"), { target: { value: "4111 19111", selectionStart: 7, selectionEnd: 7 } });
    });
    await act(async () => backend.formats[1]!.answer("4111 1911 1"));
    expect(input("Number").value).toBe("4111 1911 1");
    expect(input("Number").selectionStart).toBe(7);
  });
});
