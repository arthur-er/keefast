// The Plugins compiled into keefast, for the mock backend: each one's WASM component, the same
// `.wasm` the Rust host runs, transpiled to JavaScript by jco when the mock imports
// `virtual:keefast/bundled-plugins` (ADR 0009). Production builds don't import the mock, so they
// never run this. Keep the list in step with `BUNDLED` in crates/plugins/src/lib.rs.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import { transpileBytes } from "@bytecodealliance/jco-transpile";
import type { Plugin } from "vite";

/** `plugins/<name>/` holds `plugin.json` and `<name>.wasm`. */
const BUNDLED = ["card"];
const ROOT = fileURLToPath(new URL("../../../plugins/", import.meta.url));

const ID = "virtual:keefast/bundled-plugins";
const ONE = "\0keefast-plugin:";

/**
 * jco's code falls back to fetching the core modules beside itself, or reading them with Node's
 * `fs`; Vite would try to bundle both. The mock always passes the core modules, so neither runs:
 * take them out, and the directive bundlers warn about.
 */
function browserOnly(js: string): string {
  const fallbacks: [string, string][] = [
    ['"use components";', ""],
    ["new URL(`./${name}`, import.meta.url)", "name"],
    ["await import('node:fs/promises')", "null"],
  ];
  return fallbacks.reduce((code, [from, to]) => {
    if (!code.includes(from)) throw new Error(`jco's output changed: no ${from}`);
    return code.replaceAll(from, to);
  }, js);
}

export function bundledPlugins(): Plugin {
  return {
    name: "keefast-bundled-plugins",
    resolveId(id) {
      if (id === ID) return `\0${ID}`;
      if (id.startsWith(ONE)) return id;
      return null;
    },
    async load(id) {
      if (id === `\0${ID}`) {
        const imports = BUNDLED.map((name, index) => `import * as p${index} from "${ONE}${name}";`);
        return `${imports.join("\n")}\nexport const bundledPlugins = [${BUNDLED.map((_, index) => `p${index}`).join(", ")}];`;
      }
      if (!id.startsWith(ONE)) return null;

      const name = id.slice(ONE.length);
      const wasm = `${ROOT}${name}/${name}.wasm`;
      const manifest = `${ROOT}${name}/plugin.json`;
      this.addWatchFile(wasm);
      this.addWatchFile(manifest);
      const { files } = await transpileBytes(await readFile(wasm), {
        name,
        instantiation: "async",
        emitTypescriptDeclarations: false,
        wasiShim: false,
      });
      const js = browserOnly(new TextDecoder().decode(files[`${name}.js`]));
      // The core modules, inlined: a few tens of kB.
      const cores = Object.entries(files)
        .filter(([file]) => file.endsWith(".wasm"))
        .map(([file, bytes]) => `${JSON.stringify(file)}: ${JSON.stringify(Buffer.from(bytes).toString("base64"))}`);
      return `${js}
const cores = { ${cores.join(", ")} };
const compile = (file) => WebAssembly.compile(Uint8Array.from(atob(cores[file]), (c) => c.charCodeAt(0)));
export const manifest = ${await readFile(manifest, "utf8")};
/** A new instance of the component: it imports nothing. */
export const instantiateHooks = () => instantiate(compile, {});
`;
    },
  };
}
