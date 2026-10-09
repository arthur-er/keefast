import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import config from "../wxt.config";

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

/** The extension ID Chromium derives from a manifest's public key. */
function extensionId(key: string): string {
  const hash = createHash("sha256").update(Buffer.from(key, "base64")).digest("hex");
  return [...hash.slice(0, 32)].map((digit) => "abcdefghijklmnop"[parseInt(digit, 16)]).join("");
}

describe("manifest", () => {
  const manifest = config.manifest as Record<string, unknown>;

  it("may start keefast's native host", () => {
    const hosts = read("../../crates/workspace/src/browser_hosts.rs");

    expect(hosts).toContain(`"chrome-extension://${extensionId(manifest.key as string)}/"`);
  });

  it("injects nothing into pages by itself", () => {
    expect(manifest.content_scripts).toBeUndefined();
    expect(manifest.host_permissions).toBeUndefined();
  });

  it("runs in every page only once the user allows it", () => {
    expect(manifest.optional_host_permissions).toEqual(["<all_urls>"]);
  });
});
