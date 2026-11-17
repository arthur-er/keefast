import { describe, expect, it } from "vitest";

import type { UpdateState, UpdateStatus } from "@/bindings";

import { foundUpdate, updateNotice, updateStatusText } from "./updates";

const update = { version: "0.2.0", releasePage: "https://github.com/arthur-er/keefast/releases/tag/v0.2.0" };
const state = (status: UpdateStatus, installsUpdates = true): UpdateState => ({ currentVersion: "0.1.0", installsUpdates, status });
const none = new Set<string>();

describe("the update notice", () => {
  it("asks until the user answers, and only then", () => {
    expect(updateNotice(state({ kind: "unchecked" }), null, none)).toEqual({ kind: "ask" });
    expect(updateNotice(state({ kind: "unchecked" }), true, none)).toBeNull();
    expect(updateNotice(state({ kind: "unchecked" }), false, none)).toBeNull();
    // Settings not read yet: nothing rather than a question that may already be answered.
    expect(updateNotice(undefined, undefined, none)).toBeNull();
  });

  it("says an update was found, installable or not", () => {
    expect(updateNotice(state({ kind: "available", update }), true, none)).toEqual({ kind: "available", update, installs: true });
    expect(updateNotice(state({ kind: "available", update }, false), true, none)).toEqual({ kind: "available", update, installs: false });
  });

  it("puts a found update before the question", () => {
    // A check the user ran from Settings before answering.
    expect(updateNotice(state({ kind: "available", update }), null, none)?.kind).toBe("available");
  });

  it("stays closed for the version dismissed", () => {
    const dismissed = new Set(["0.2.0"]);
    expect(updateNotice(state({ kind: "available", update }), true, dismissed)).toBeNull();
    expect(updateNotice(state({ kind: "available", update: { ...update, version: "0.3.0" } }), true, dismissed)?.kind).toBe("available");
  });

  it("shows a download in progress, even when dismissed", () => {
    expect(updateNotice(state({ kind: "downloading", update, progress: 0.4 }), true, new Set(["0.2.0"]))).toEqual({ kind: "downloading", update, progress: 0.4 });
  });

  it("says an install failed, but not a check that failed unattended", () => {
    const message = "keefast couldn't download the update: timed out";
    expect(updateNotice(state({ kind: "failed", message, update }), true, none)).toEqual({ kind: "failed", update, message });
    expect(updateNotice(state({ kind: "failed", message: "keefast couldn't check for updates: offline", update: null }), true, none)).toBeNull();
  });
});

describe("Settings' Version row", () => {
  it("says where updating stands", () => {
    expect(updateStatusText(state({ kind: "unchecked" }))).toBe("keefast 0.1.0");
    expect(updateStatusText(state({ kind: "checking" }))).toBe("Checking for updates…");
    const now = Date.UTC(2026, 10, 17, 12);
    expect(updateStatusText(state({ kind: "upToDate", checkedAt: now / 1000 - 300 }), now)).toBe("keefast 0.1.0 is up to date · checked 5 min ago");
    expect(updateStatusText(state({ kind: "available", update }))).toBe("keefast 0.2.0 is available");
    expect(updateStatusText(state({ kind: "available", update }, false))).toBe("keefast 0.2.0 is available: download it from its release page");
    expect(updateStatusText(state({ kind: "downloading", update, progress: 0.426 }))).toBe("Downloading keefast 0.2.0… 43%");
    expect(updateStatusText(state({ kind: "downloading", update, progress: null }))).toBe("Downloading keefast 0.2.0…");
  });

  it("knows the update found", () => {
    expect(foundUpdate({ kind: "available", update })).toBe(update);
    expect(foundUpdate({ kind: "failed", message: "", update })).toBe(update);
    expect(foundUpdate({ kind: "failed", message: "", update: null })).toBeNull();
    expect(foundUpdate({ kind: "upToDate", checkedAt: 0 })).toBeNull();
  });
});
