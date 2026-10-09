// Builds the unpacked extension into dist/: load it from there in chrome://extensions.
import { cpSync, rmSync } from "node:fs";
import { build } from "esbuild";

rmSync("dist", { recursive: true, force: true });
cpSync("static", "dist", { recursive: true });
await build({
  entryPoints: ["src/background.ts", "src/content.ts", "src/popup.ts"],
  outdir: "dist",
  bundle: true,
  format: "iife",
  target: "chrome127",
  logLevel: "warning",
});
