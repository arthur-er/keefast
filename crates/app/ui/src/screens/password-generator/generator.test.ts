import { describe, expect, it } from "vitest";

import type { GeneratorOptions } from "@/bindings";

import { classCount, segments, sliderRange, toggleClass } from "./generator";

const options: GeneratorOptions = {
  mode: "characters",
  characters: { length: 3, lowercase: true, uppercase: false, digits: false, symbols: false, excludeLookalikes: false },
  passphrase: { words: 6, separator: "-" },
};

describe("segments", () => {
  it("colours digits and symbols apart from letters", () => {
    expect(segments("ab12!?c", options)).toEqual([
      { text: "ab", kind: "plain" },
      { text: "12", kind: "digit" },
      { text: "!?", kind: "symbol" },
      { text: "c", kind: "plain" },
    ]);
  });

  it("shows a passphrase's separators apart from its words", () => {
    expect(segments("maple-orbit", { ...options, mode: "passphrase" })).toEqual([
      { text: "maple", kind: "plain" },
      { text: "-", kind: "separator" },
      { text: "orbit", kind: "plain" },
    ]);
  });
});

describe("toggleClass", () => {
  it("grows the length to fit every class turned on", () => {
    const one = { ...options, characters: { ...options.characters, length: 1 } };
    const next = toggleClass(toggleClass(one, "digits"), "symbols");
    expect(classCount(next)).toBe(3);
    expect(next.characters.length).toBe(3);
  });

  it("can turn every class off, which the generator then refuses", () => {
    expect(classCount(toggleClass(options, "lowercase"))).toBe(0);
  });
});

describe("sliderRange", () => {
  it("covers lengths from one per class up to 64, and up to 12 words", () => {
    const limits = { maxLength: 256, maxWords: 40, symbols: "", lookalikes: "" };
    expect(sliderRange(toggleClass(options, "digits"), limits)).toEqual({ min: 2, max: 64 });
    expect(sliderRange({ ...options, mode: "passphrase" }, limits)).toEqual({ min: 1, max: 12 });
  });
});
