// The module bundled-plugins.ts makes: the Plugins compiled into keefast, run through jco.
declare module "virtual:keefast/bundled-plugins" {
  import type { PluginHooks, PluginManifest } from "@/api/mock/plugins";

  export const bundledPlugins: readonly {
    manifest: PluginManifest;
    /** A new instance of the Plugin's component. */
    instantiateHooks(): Promise<PluginHooks>;
  }[];
}
