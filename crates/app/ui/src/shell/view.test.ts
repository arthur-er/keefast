import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { View } from "@/bindings";

// The backend, held in the test's hands: `setView` answers when told to, and the snapshot is
// whatever the test says was fetched last.
const backend = vi.hoisted(() => ({
  calls: [] as { view: unknown; answer: (revision: number) => void }[],
  snapshot: { revision: 0, view: null as unknown },
}));

vi.mock("@/api", () => ({
  api: {
    setView: (view: unknown) => new Promise<number>((answer) => backend.calls.push({ view, answer })),
  },
  errorMessage: String,
}));
vi.mock("@/api/queries", () => ({ useWorkspace: () => ({ data: backend.snapshot }) }));
vi.mock("@/kit", () => ({ toast: vi.fn() }));

const { showView, useView } = await import("./view");

const at = (entry: string): View => ({ scope: { kind: "vault", vault: 0 }, entry: { vault: 0, entry } });

describe("the view shown while the backend catches up", () => {
  beforeEach(() => {
    backend.calls = [];
    backend.snapshot = { revision: 5, view: at("e3") };
  });

  it("doesn't jump to an Entry passed through when the selection comes straight back", async () => {
    const { result, rerender } = renderHook(() => useView());
    const shown: (string | undefined)[] = [];
    const fetched = (revision: number, view: View) => {
      backend.snapshot = { revision, view };
      rerender();
      shown.push(result.current?.entry?.entry);
    };

    act(() => showView(at("e2"))); // down
    act(() => showView(at("e3"))); // and straight back up
    expect(result.current).toEqual(at("e3"));
    // One at a time: the second waits for the first, so they reach the backend in order.
    expect(backend.calls.map((call) => call.view)).toEqual([at("e2")]);

    // A snapshot read before either change shows e3, which isn't the backend catching up.
    fetched(5, at("e3"));
    await act(async () => backend.calls[0]!.answer(6));
    expect(backend.calls.map((call) => call.view)).toEqual([at("e2"), at("e3")]);
    // Then the snapshot of the first change: e2, passed through.
    fetched(6, at("e2"));
    await act(async () => backend.calls[1]!.answer(7));
    fetched(7, at("e3"));

    expect(shown).toEqual(["e3", "e3", "e3"]);
  });

  it("shows the backend's view once it has caught up, even when something since moved it", async () => {
    const { result, rerender } = renderHook(() => useView());

    act(() => showView(at("e7")));
    await act(async () => backend.calls[0]!.answer(6));
    expect(result.current).toEqual(at("e7"));

    // The Entry was deleted after the view changed: the backend's view wins.
    backend.snapshot = { revision: 7, view: at("e1") };
    rerender();
    expect(result.current).toEqual(at("e1"));
  });
});
