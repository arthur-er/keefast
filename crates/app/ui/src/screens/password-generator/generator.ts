// The password generator's preview and controls, as plain data.

import type { GeneratorLimits, GeneratorOptions } from "@/bindings";

export type Segment = { text: string; kind: "plain" | "digit" | "symbol" | "separator" };

/** The password in runs to colour as the prototype does: digits, symbols, and a passphrase's
 * separators apart from the letters. */
export function segments(password: string, options: Pick<GeneratorOptions, "mode" | "passphrase">): Segment[] {
  const out: Segment[] = [];
  const push = (text: string, kind: Segment["kind"]) => {
    const last = out[out.length - 1];
    if (last && last.kind === kind) last.text += text;
    else out.push({ text, kind });
  };
  if (options.mode === "passphrase") {
    const separator = options.passphrase.separator;
    const words = separator ? password.split(separator) : [password];
    words.forEach((word, index) => {
      if (index > 0) push(separator, "separator");
      if (word) push(word, "plain");
    });
    return out;
  }
  for (const char of password) push(char, /[0-9]/.test(char) ? "digit" : /[\p{L}]/u.test(char) ? "plain" : "symbol");
  return out;
}

/** The character classes turned on. */
export function classCount(options: GeneratorOptions): number {
  const c = options.characters;
  return [c.lowercase, c.uppercase, c.digits, c.symbols].filter(Boolean).length;
}

/** The range the slider covers: the length up to 64 (the field takes up to the maximum), or
 * the words up to 12. */
export function sliderRange(options: GeneratorOptions, limits: GeneratorLimits | undefined): { min: number; max: number } {
  if (options.mode === "passphrase") return { min: 1, max: Math.min(12, limits?.maxWords ?? 12) };
  return { min: Math.max(1, classCount(options)), max: Math.min(64, limits?.maxLength ?? 64) };
}

/** The options after turning a character class on or off: the length grows to fit every class
 * on, as each class gives at least one character. */
export function toggleClass(options: GeneratorOptions, key: "lowercase" | "uppercase" | "digits" | "symbols"): GeneratorOptions {
  const characters = { ...options.characters, [key]: !options.characters[key] };
  const next = { ...options, characters };
  const min = classCount(next);
  if (characters.length < min) characters.length = min;
  return next;
}

/** The separators offered as one-click chips; `␣` is a space. */
export const SEPARATORS = ["-", ".", "_", " "] as const;
