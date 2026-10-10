// The encryption part of the master key forms (New Vault, and changing a Vault's master key):
// the cipher and key derivation as the user edits them, as text, and back to the settings the
// backend takes, checked as key derivation would check them.

import type { CipherChoice, EncryptionSettings, KdfChoice } from "@/bindings";

export type KdfKind = KdfChoice["kind"];

export const CIPHERS: readonly { value: CipherChoice; label: string }[] = [
  { value: "aes256", label: "AES-256" },
  { value: "chaCha20", label: "ChaCha20" },
  { value: "twofish", label: "Twofish" },
];

export const KDFS: readonly { value: KdfKind; label: string }[] = [
  { value: "argon2id", label: "Argon2id" },
  { value: "argon2d", label: "Argon2d" },
  { value: "aesKdf", label: "AES-KDF" },
];

/** What the user edits: the numbers as typed. Argon2's memory is in MiB. */
export interface EncryptionForm {
  cipher: CipherChoice;
  kdf: KdfKind;
  memoryMib: string;
  iterations: string;
  parallelism: string;
  rounds: string;
}

export type EncryptionField = "memoryMib" | "iterations" | "parallelism" | "rounds";

export type Checked =
  | { ok: true; settings: EncryptionSettings }
  | { ok: false; message: string; /** The field to mark, if one is to blame. */ field: EncryptionField | null };

/** Argon2's settings when switching to it from AES-KDF: keefast's default cost. */
const DEFAULT_ARGON2 = { memoryKib: 64 * 1024, iterations: 5, parallelism: 4 };
/** AES-KDF's rounds when switching to it: about a second's worth, as KeePass suggests. */
const DEFAULT_AES_ROUNDS = 2_000_000;

const MAX_U32 = 4_294_967_295;
const MAX_LANES = 0xff_ffff;
const MIN_KIB_PER_LANE = 8;

export const MESSAGES = {
  number: "Enter whole numbers for the key derivation settings.",
  iterations: "Argon2 needs at least 1 iteration.",
  parallelism: "Argon2's parallelism must be between 1 and 16777215.",
  memory: "Argon2's memory must be at least 8 KiB per unit of parallelism, and at most 4 TiB.",
  rounds: "AES-KDF needs at least 1 round.",
  roundsMax: "AES-KDF can't take more than 4294967295 rounds.",
} as const;

/** Argon2's memory in whole MiB, rounded up, as the form shows it. */
export function mebibytes(kib: number): string {
  return String(Math.ceil(kib / 1024));
}

export function encryptionForm(settings: EncryptionSettings): EncryptionForm {
  const kdf = settings.kdf;
  const argon2 = kdf.kind === "aesKdf" ? DEFAULT_ARGON2 : kdf;
  return {
    cipher: settings.cipher,
    kdf: kdf.kind,
    memoryMib: mebibytes(argon2.memoryKib),
    iterations: String(argon2.iterations),
    parallelism: String(argon2.parallelism),
    rounds: String(kdf.kind === "aesKdf" ? kdf.rounds : DEFAULT_AES_ROUNDS),
  };
}

/** The settings the form describes, or why they can't be used. Argon2's memory stays exactly as
 * it was in `initial` (KiB, which the form shows rounded to MiB) unless its field changed. */
export function checkEncryption(form: EncryptionForm, initial: EncryptionSettings): Checked {
  const fail = (message: string, field: EncryptionField | null): Checked => ({ ok: false, message, field });
  const fields: EncryptionField[] = form.kdf === "aesKdf" ? ["rounds"] : ["memoryMib", "iterations", "parallelism"];
  const bad = fields.find((field) => !/^\d+$/.test(form[field].trim()));
  if (bad) return fail(MESSAGES.number, bad);
  const number = (field: EncryptionField) => Number(form[field].trim());

  if (form.kdf === "aesKdf") {
    const rounds = number("rounds");
    if (rounds < 1) return fail(MESSAGES.rounds, "rounds");
    if (rounds > MAX_U32) return fail(MESSAGES.roundsMax, "rounds");
    return { ok: true, settings: { cipher: form.cipher, kdf: { kind: "aesKdf", rounds } } };
  }

  const iterations = number("iterations");
  if (iterations < 1 || iterations > MAX_U32) return fail(MESSAGES.iterations, "iterations");
  const parallelism = number("parallelism");
  if (parallelism < 1 || parallelism > MAX_LANES) return fail(MESSAGES.parallelism, "parallelism");
  const kept = initial.kdf.kind !== "aesKdf" && form.memoryMib.trim() === mebibytes(initial.kdf.memoryKib);
  const memoryKib = kept && initial.kdf.kind !== "aesKdf" ? initial.kdf.memoryKib : number("memoryMib") * 1024;
  if (memoryKib < MIN_KIB_PER_LANE * parallelism || memoryKib > MAX_U32) return fail(MESSAGES.memory, "memoryMib");
  return { ok: true, settings: { cipher: form.cipher, kdf: { kind: form.kdf, memoryKib, iterations, parallelism } } };
}

export function cipherName(cipher: CipherChoice): string {
  return CIPHERS.find((option) => option.value === cipher)?.label ?? cipher;
}

export function kdfName(kdf: KdfKind): string {
  return KDFS.find((option) => option.value === kdf)?.label ?? kdf;
}

/** The settings in a few words, as New Vault's summary shows them: AES-256, Argon2id, 64 MiB, 5×. */
export function encryptionSummary(settings: EncryptionSettings): string[] {
  const kdf = settings.kdf;
  const parts = [cipherName(settings.cipher), kdfName(kdf.kind)];
  if (kdf.kind === "aesKdf") parts.push(`${kdf.rounds.toLocaleString("en")} rounds`);
  else parts.push(`${mebibytes(kdf.memoryKib)} MiB`, `${kdf.iterations}×`);
  return parts;
}

/** The summary of what the form says now, or of `fallback` while the form can't be used. */
export function formSummary(form: EncryptionForm, fallback: EncryptionSettings): string[] {
  const checked = checkEncryption(form, fallback);
  if (checked.ok) return encryptionSummary(checked.settings);
  const parts = [cipherName(form.cipher), kdfName(form.kdf)];
  if (form.kdf === "aesKdf") parts.push(`${form.rounds.trim() || "?"} rounds`);
  else parts.push(`${form.memoryMib.trim() || "?"} MiB`, `${form.iterations.trim() || "?"}×`);
  return parts;
}
