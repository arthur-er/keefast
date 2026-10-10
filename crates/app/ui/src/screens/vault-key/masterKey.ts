// The master key part of the master key forms: a master password typed twice, a Key file, or
// both.

import type { KeyInput } from "@/bindings";

export const KEY_MESSAGES = {
  mismatch: "The two master passwords are different.",
  none: "Set a master password, a Key file, or both.",
} as const;

export type KeyField = "password" | "confirm";

export type CheckedKey = { ok: true; key: KeyInput } | { ok: false; message: string; field: KeyField };

/** The master key typed, or why it can't be used. */
export function checkKey(password: string, confirm: string, keyFile: string | null): CheckedKey {
  if (!password && !keyFile) return { ok: false, message: KEY_MESSAGES.none, field: "password" };
  if (password !== confirm) return { ok: false, message: KEY_MESSAGES.mismatch, field: "confirm" };
  return { ok: true, key: { password, keyFile } };
}

/** What to say under the repeated master password while it is typed: nothing while it could
 * still become the first, that they match, or that they differ. */
export function matchState(password: string, confirm: string): "none" | "match" | "mismatch" {
  if (!confirm) return "none";
  if (confirm === password) return "match";
  return password.startsWith(confirm) ? "none" : "mismatch";
}

/** A Key file's name, from its path. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}
