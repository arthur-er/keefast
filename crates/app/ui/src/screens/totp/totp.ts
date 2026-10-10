// TOTP B's setup form, as plain data: what an otpauth:// URI says (to show the settings it
// brings), and which control the backend's complaint is about.

import type { TotpAlgorithmChoice, TotpSetup } from "@/bindings";

export interface UriSettings {
  issuer: string | null;
  account: string | null;
  digits: string | null;
  period: string | null;
  algorithm: TotpAlgorithmChoice | null;
}

/** The settings an otpauth:// URI gives, for the form to show; `null` when the text isn't one.
 * The backend checks it for real. */
export function uriSettings(text: string): UriSettings | null {
  const trimmed = text.trim();
  if (!/^otpauth:\/\//i.test(trimmed)) return null;
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return { issuer: null, account: null, digits: null, period: null, algorithm: null };
  }
  const label = decodeURIComponent(url.pathname.replace(/^\/+/, ""));
  const colon = label.indexOf(":");
  const params = url.searchParams;
  const algorithm = params.get("algorithm")?.toLowerCase().replace("-", "");
  return {
    issuer: params.get("issuer") || (colon > 0 ? label.slice(0, colon) : null),
    account: (colon >= 0 ? label.slice(colon + 1) : label).trim() || null,
    digits: params.get("digits"),
    period: params.get("period"),
    algorithm: algorithm === "sha1" || algorithm === "sha256" || algorithm === "sha512" ? algorithm : null,
  };
}

/** Which control a TOTP error is about, from the backend's message ("A code has 1 to 10
 * digits.", "The period is…"); anything else is about the secret key or URI. */
export function errorField(message: string): "secret" | "digits" | "period" {
  if (/^a code has/i.test(message)) return "digits";
  if (/^the period/i.test(message)) return "period";
  return "secret";
}

export const ALGORITHMS: readonly { value: TotpAlgorithmChoice; label: string }[] = [
  { value: "sha1", label: "SHA-1" },
  { value: "sha256", label: "SHA-256" },
  { value: "sha512", label: "SHA-512" },
];

export const EMPTY_SETUP: TotpSetup = { secretOrUri: "", algorithm: "sha1", digits: "6", period: "30" };

/** A code split in two halves, "482 913", as DESIGN.md shows TOTP codes. */
export function splitCode(code: string): [string, string] {
  const half = Math.ceil(code.length / 2);
  return [code.slice(0, half), code.slice(half)];
}
