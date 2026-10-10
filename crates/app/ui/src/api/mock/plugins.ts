// The mock's Plugin host: what crates/plugins does for the app, over the same components run
// through jco (bundled-plugins.ts). It finds the facets an Entry has by its custom fields' names,
// asks each Plugin's `derive` for labels, and answers in `FacetView`s, protected values left out.
// No fuel or memory limits: the mock trusts the Plugins compiled into keefast.

import { bundledPlugins } from "virtual:keefast/bundled-plugins";

import type { FacetView, OwnedFieldKind } from "@/bindings";

import type { MockField } from "./data";

/** A Plugin's manifest, `plugins/<name>/plugin.json`; crates/plugins/src/manifest.rs reads it. */
export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  apiVersion: number;
  capabilities: ("readProtected" | "copy")[];
  contributions: {
    facets: {
      id: string;
      title: string;
      icon?: string;
      detect: { fields: string[] };
      fields: { key: string; name: string; label: string; kind: OwnedFieldKind; protected?: boolean }[];
    }[];
  };
}

/** The hooks of the `plugin` world (crates/plugins/wit/plugin.wit), as jco exposes them. */
export interface PluginHooks {
  format(facet: string, key: string, value: string): string;
  validate(facet: string, key: string, value: string): { severity: "warning" | "error"; message: string } | undefined;
  derive(facet: string, fields: { key: string; value?: string }[]): { label?: string; fields: { key: string; text: string }[] };
}

/** As the host's `MAX_LABEL`: longer text is cut. */
const MAX_LABEL = 64;
const shorten = (text: string) => {
  const line = text.split("\n")[0] ?? "";
  const chars = [...line];
  return chars.length > MAX_LABEL ? `${chars.slice(0, MAX_LABEL - 1).join("")}…` : line;
};

const instances = new Map<string, Promise<PluginHooks>>();
function hooksOf(id: string): Promise<PluginHooks> {
  let hooks = instances.get(id);
  if (!hooks) {
    const plugin = bundledPlugins.find((candidate) => candidate.manifest.id === id);
    if (!plugin) return Promise.reject(new Error(`no Plugin ${id}`));
    hooks = plugin.instantiateHooks();
    instances.set(id, hooks);
  }
  return hooks;
}

/** The facets an Entry with these custom fields has, labelled by their Plugins. */
export async function facetsOf(fields: readonly MockField[]): Promise<FacetView[]> {
  const indexOf = (name: string) => fields.findIndex((field) => field.name === name);
  const found: FacetView[] = [];
  for (const { manifest } of bundledPlugins) {
    for (const facet of manifest.contributions.facets) {
      if (!facet.detect.fields.every((name) => indexOf(name) >= 0)) continue;
      const indexes = facet.fields.map((owned) => indexOf(owned.name));
      const readProtected = manifest.capabilities.includes("readProtected");
      const inputs = facet.fields.map((owned, at) => {
        const kept = fields[indexes[at]!];
        return { key: owned.key, value: kept && (!kept.protected || readProtected) ? kept.value : undefined };
      });
      let derived: ReturnType<PluginHooks["derive"]> | null = null;
      try {
        derived = (await hooksOf(manifest.id)).derive(facet.id, inputs);
      } catch (error) {
        console.warn(`keefast mock: the ${manifest.name} Plugin failed`, error);
      }
      const text = (key: string) => {
        const given = derived?.fields.find((field) => field.key === key);
        return given ? shorten(given.text) : null;
      };
      found.push({
        plugin: manifest.id,
        facet: facet.id,
        title: facet.title,
        icon: facet.icon ?? null,
        label: derived?.label !== undefined ? shorten(derived.label) : null,
        canCopy: manifest.capabilities.includes("copy"),
        fields: facet.fields.map((owned, at) => {
          const index = indexes[at]!;
          const kept = fields[index];
          const isProtected = kept ? kept.protected : (owned.protected ?? false);
          return {
            key: owned.key,
            name: owned.name,
            label: owned.label,
            kind: owned.kind,
            index: kept ? index : null,
            protected: isProtected,
            value: kept && !isProtected ? kept.value : null,
            isEmpty: !kept || kept.value === "",
            text: text(owned.key),
          };
        }),
      });
    }
  }
  return found;
}
