import { describe, expect, it } from "vitest";

import type { EncryptionSettings } from "@/bindings";

import { checkEncryption, encryptionForm, encryptionSummary, formSummary, MESSAGES } from "./encryption";
import { checkKey, KEY_MESSAGES, matchState } from "./masterKey";

const DEFAULT: EncryptionSettings = { cipher: "aes256", kdf: { kind: "argon2id", memoryKib: 65536, iterations: 5, parallelism: 4 } };

describe("the encryption form", () => {
  it("gives back the settings it started from", () => {
    expect(checkEncryption(encryptionForm(DEFAULT), DEFAULT)).toEqual({ ok: true, settings: DEFAULT });
  });

  it("keeps Argon2's memory exactly while its MiB field is untouched", () => {
    const odd: EncryptionSettings = { cipher: "chaCha20", kdf: { kind: "argon2d", memoryKib: 1000, iterations: 2, parallelism: 1 } };
    const form = encryptionForm(odd);
    expect(form.memoryMib).toBe("1");
    expect(checkEncryption({ ...form, iterations: "3" }, odd)).toEqual({
      ok: true,
      settings: { cipher: "chaCha20", kdf: { kind: "argon2d", memoryKib: 1000, iterations: 3, parallelism: 1 } },
    });
    expect(checkEncryption({ ...form, memoryMib: "2" }, odd)).toMatchObject({ ok: true, settings: { kdf: { memoryKib: 2048 } } });
  });

  it("switches to AES-KDF with a second's worth of rounds, and back to Argon2's defaults", () => {
    const aes = { ...encryptionForm(DEFAULT), kdf: "aesKdf" as const };
    expect(checkEncryption(aes, DEFAULT)).toEqual({ ok: true, settings: { cipher: "aes256", kdf: { kind: "aesKdf", rounds: 2_000_000 } } });
    const fromAes = encryptionForm({ cipher: "twofish", kdf: { kind: "aesKdf", rounds: 60_000 } });
    expect(fromAes).toMatchObject({ kdf: "aesKdf", rounds: "60000", memoryMib: "64", iterations: "5", parallelism: "4" });
  });

  it("refuses what key derivation would, and says which field", () => {
    const form = encryptionForm(DEFAULT);
    expect(checkEncryption({ ...form, iterations: "five" }, DEFAULT)).toEqual({ ok: false, message: MESSAGES.number, field: "iterations" });
    expect(checkEncryption({ ...form, iterations: "0" }, DEFAULT)).toMatchObject({ message: MESSAGES.iterations, field: "iterations" });
    expect(checkEncryption({ ...form, parallelism: "0" }, DEFAULT)).toMatchObject({ message: MESSAGES.parallelism });
    expect(checkEncryption({ ...form, parallelism: "16777216" }, DEFAULT)).toMatchObject({ message: MESSAGES.parallelism });
    expect(checkEncryption({ ...form, memoryMib: "0" }, DEFAULT)).toMatchObject({ message: MESSAGES.memory, field: "memoryMib" });
    expect(checkEncryption({ ...form, memoryMib: "4194304" }, DEFAULT)).toMatchObject({ message: MESSAGES.memory });
    expect(checkEncryption({ ...form, kdf: "aesKdf", rounds: "0" }, DEFAULT)).toMatchObject({ message: MESSAGES.rounds, field: "rounds" });
    expect(checkEncryption({ ...form, kdf: "aesKdf", rounds: "4294967296" }, DEFAULT)).toMatchObject({ message: MESSAGES.roundsMax });
    // Fields the chosen KDF doesn't use don't matter.
    expect(checkEncryption({ ...form, kdf: "aesKdf", iterations: "x" }, DEFAULT)).toMatchObject({ ok: true });
  });

  it("sums the settings up in a few words", () => {
    expect(encryptionSummary(DEFAULT)).toEqual(["AES-256", "Argon2id", "64 MiB", "5×"]);
    expect(encryptionSummary({ cipher: "twofish", kdf: { kind: "aesKdf", rounds: 2_000_000 } })).toEqual(["Twofish", "AES-KDF", "2,000,000 rounds"]);
    expect(formSummary({ ...encryptionForm(DEFAULT), memoryMib: "" }, DEFAULT)).toEqual(["AES-256", "Argon2id", "? MiB", "5×"]);
  });
});

describe("the master key", () => {
  it("needs a master password, a Key file, or both", () => {
    expect(checkKey("", "", null)).toEqual({ ok: false, message: KEY_MESSAGES.none, field: "password" });
    expect(checkKey("", "", "/k.keyx")).toEqual({ ok: true, key: { password: "", keyFile: "/k.keyx" } });
    expect(checkKey("pw", "pw", null)).toEqual({ ok: true, key: { password: "pw", keyFile: null } });
  });

  it("needs the master password typed the same twice", () => {
    expect(checkKey("pw", "pW", null)).toEqual({ ok: false, message: KEY_MESSAGES.mismatch, field: "confirm" });
  });

  it("says nothing about the repeat while it could still match", () => {
    expect(matchState("secret", "")).toBe("none");
    expect(matchState("secret", "sec")).toBe("none");
    expect(matchState("secret", "secret")).toBe("match");
    expect(matchState("secret", "sex")).toBe("mismatch");
  });
});
