// The mock's Plugin host: what crates/plugins does for the app, over the same components run
// through jco (bundled-plugins.ts). It finds the facets an Entry has by its custom fields' names,
// asks each Plugin's `derive` for labels, and answers in `FacetView`s, protected values left out.
// For Entry editor A it lists the facets Plugins contribute, and runs `format` and `validate`
// (passing the month it is, as the app does: Plugins have no clock). No fuel or memory limits:
// the mock trusts the Plugins compiled into keefast.

import { bundledPlugins } from "virtual:keefast/bundled-plugins";

import type { FacetTemplate, FacetView, OwnedFieldKind, ProblemView } from "@/bindings";

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
      fields: { key: string; name: string; label: string; kind: OwnedFieldKind; protected?: boolean; placeholder?: string }[];
      editor?: { add: string; remove: string };
    }[];
  };
}

/** The hooks of the `plugin` world (crates/plugins/wit/plugin.wit), as jco exposes them. */
export interface PluginHooks {
  format(facet: string, key: string, value: string): string;
  validate(
    facet: string,
    key: string,
    value: string,
    today: { year: number; month: number },
  ): { severity: "warning" | "error"; message: string } | undefined;
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

/** The facets an Entry with these custom fields has, labelled by their Plugins: those of the
 * Plugins that `runs` (a disabled one finds none). */
export async function facetsOf(fields: readonly MockField[], runs: (id: string) => boolean = () => true): Promise<FacetView[]> {
  const indexOf = (name: string) => fields.findIndex((field) => field.name === name);
  const found: FacetView[] = [];
  for (const { manifest } of bundledPlugins) {
    if (!runs(manifest.id)) continue;
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
        detect: facet.detect.fields,
        editor: facet.editor ?? null,
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
            placeholder: owned.placeholder ?? null,
          };
        }),
      });
    }
  }
  return found;
}

/** Every facet the Plugins that `runs` contribute, as `list_facets` answers. */
export function facetTemplates(runs: (id: string) => boolean = () => true): FacetTemplate[] {
  return bundledPlugins.flatMap(({ manifest }) =>
    (runs(manifest.id) ? manifest.contributions.facets : []).map((facet) => ({
      plugin: manifest.id,
      facet: facet.id,
      title: facet.title,
      icon: facet.icon ?? null,
      detect: facet.detect.fields,
      editor: facet.editor ?? null,
      fields: facet.fields.map((owned) => ({
        key: owned.key,
        name: owned.name,
        label: owned.label,
        kind: owned.kind,
        protected: owned.protected ?? false,
        placeholder: owned.placeholder ?? null,
      })),
    })),
  );
}

/** A running Plugin's facet, checked to exist, as the host's `facet_of`. */
async function facetHooks(plugin: string, facet: string, runs: (id: string) => boolean): Promise<PluginHooks | null> {
  const found = bundledPlugins.find((candidate) => candidate.manifest.id === plugin && runs(plugin));
  if (!found?.manifest.contributions.facets.some((candidate) => candidate.id === facet)) return null;
  return hooksOf(plugin);
}

/** A value typed into an Owned field, formatted by its Plugin; `null` for no such facet. */
export async function formatOwned(
  plugin: string,
  facet: string,
  key: string,
  value: string,
  runs: (id: string) => boolean = () => true,
): Promise<string | null> {
  const hooks = await facetHooks(plugin, facet, runs);
  return hooks ? hooks.format(facet, key, value) : null;
}

/** The month `now` is in, in UTC, as the app passes `validate`. */
export const monthOf = (now: Date) => ({ year: now.getUTCFullYear(), month: now.getUTCMonth() + 1 });

/** What the Plugin says is wrong with a value; `undefined` for no such facet. */
export async function validateOwned(
  plugin: string,
  facet: string,
  key: string,
  value: string,
  runs: (id: string) => boolean = () => true,
  now = new Date(),
): Promise<ProblemView | null | undefined> {
  const hooks = await facetHooks(plugin, facet, runs);
  if (!hooks) return undefined;
  const problem = hooks.validate(facet, key, value, monthOf(now));
  // As the host, which cuts a message to 200 characters.
  return problem ? { severity: problem.severity, message: [...problem.message.split("\n")[0]!].slice(0, 200).join("") } : null;
}
