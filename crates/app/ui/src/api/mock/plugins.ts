// The mock's Plugin host: what crates/plugins does for the app, over the same components run
// through jco (bundled-plugins.ts). It finds the facets an Entry has by its custom fields' names,
// asks each Plugin's `derive` for labels, and answers in `FacetView`s, protected values left out.
// For Entry editor A it lists the facets Plugins contribute, and runs `format` and `validate`
// (passing the month it is, as the app does: Plugins have no clock). For the Entry list, a
// facet's label is the subtitle of an Entry with no username; for the command palette, a facet's
// commands get the shortcuts keefast and earlier Plugins haven't taken, as the host checks them
// (crates/plugins/src/shortcut.rs). No fuel or memory limits: the mock trusts the Plugins
// compiled into keefast.

import { bundledPlugins } from "virtual:keefast/bundled-plugins";

import type { FacetTemplate, FacetView, OwnedFieldKind, ProblemView } from "@/bindings";
import { SHORTCUTS } from "@/lib/shortcuts";

import type { MockField } from "./data";

/** A Plugin's manifest, `plugins/<name>/plugin.json`; crates/plugins/src/manifest.rs reads it. */
export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  apiVersion: number;
  capabilities: ("readProtected" | "copy" | "browserFill")[];
  contributions: {
    facets: {
      id: string;
      title: string;
      icon?: string;
      detect: { fields: string[] };
      fields: { key: string; name: string; label: string; kind: OwnedFieldKind; protected?: boolean; placeholder?: string }[];
      editor?: { add: string; remove: string };
      subtitle?: boolean;
      commands?: { id: string; title: string; copy: string; shortcut?: string }[];
      /** What the Companion extension fills from the facet: Owned field keys to `autocomplete`
       * tokens. */
      fill?: { form: "payment"; fields: Record<string, string> };
    }[];
  };
}

/** The Owned fields the Companion extension may fill into web forms, by name, as the host's
 * `Manifest::filled_names`. */
export function filledNames(manifest: PluginManifest): string[] {
  return manifest.contributions.facets.flatMap((facet) => facet.fields.filter((field) => facet.fill?.fields[field.key] !== undefined).map((field) => field.name));
}

/** The kinds of form it fills, each once. */
export function fillForms(manifest: PluginManifest): "payment"[] {
  return [...new Set(manifest.contributions.facets.flatMap((facet) => (facet.fill ? [facet.fill.form] : [])))];
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

/** Keys every text field owns, as the host's `TEXT_KEYS`: reserved even though `SHORTCUTS`
 * doesn't name them. */
const TEXT_KEYS = ["Ctrl A", "Ctrl C", "Ctrl V", "Ctrl X", "Ctrl Y", "Ctrl Z", "Ctrl Shift Z"];

/** A combo as the host compares them ("Ctrl Shift C"), "Mod" counting as Ctrl; `null` without a
 * key. */
function canonical(text: string): { ctrl: boolean; shift: boolean; alt: boolean; meta: boolean; key: string; name: string } | null {
  const combo = { ctrl: false, shift: false, alt: false, meta: false, key: "" };
  for (const part of text.split(/[\s+]+/).filter(Boolean)) {
    const lower = part.toLowerCase();
    if (lower === "ctrl" || lower === "control" || lower === "mod") combo.ctrl = true;
    else if (lower === "shift") combo.shift = true;
    else if (lower === "alt" || lower === "option") combo.alt = true;
    else if (lower === "cmd" || lower === "meta" || lower === "super") combo.meta = true;
    else combo.key = lower;
  }
  if (!combo.key) return null;
  const parts = [combo.ctrl && "Ctrl", combo.meta && "Meta", combo.alt && "Alt", combo.shift && "Shift"].filter(Boolean);
  return { ...combo, name: [...parts, [...combo.key].length === 1 ? combo.key.toUpperCase() : combo.key].join(" ") };
}

/** A shortcut a Plugin declared and didn't get, and why, as the host says it. */
export interface RefusedShortcut {
  plugin: string;
  facet: string;
  command: string;
  reason: string;
}

/** The shortcuts each Plugin's commands get, by `plugin/facet/command`, as the host gives them
 * at load: Ctrl with a letter or digit, or Ctrl Shift with a letter, that neither keefast
 * (`reserved` and the keys text fields own) nor a Plugin loaded earlier has. */
export function giveShortcuts(
  manifests: readonly PluginManifest[],
  reserved: readonly string[],
): { given: Map<string, string>; refused: RefusedShortcut[] } {
  const taken = new Set([...reserved, ...TEXT_KEYS].map((text) => canonical(text)?.name));
  const owners = new Map<string, string>();
  const given = new Map<string, string>();
  const refused: RefusedShortcut[] = [];
  for (const manifest of manifests) {
    for (const facet of manifest.contributions.facets) {
      for (const command of facet.commands ?? []) {
        if (command.shortcut === undefined) continue;
        const refuse = (reason: string) => refused.push({ plugin: manifest.id, facet: facet.id, command: command.id, reason });
        const combo = canonical(command.shortcut);
        const single = combo && [...combo.key].length === 1;
        const letter = single && /[a-z]/.test(combo.key);
        const digit = single && /[0-9]/.test(combo.key);
        if (!combo || !combo.ctrl || combo.alt || combo.meta || !(letter || (digit && !combo.shift))) {
          refuse(`"${command.shortcut}" isn't a shortcut a Plugin can have: use Ctrl with a letter or a digit, or Ctrl Shift with a letter`);
        } else if (taken.has(combo.name)) {
          refuse(`${combo.name} is one of keefast's own shortcuts`);
        } else if (owners.has(combo.name)) {
          refuse(`${combo.name} is taken by the ${owners.get(combo.name)} Plugin`);
        } else {
          owners.set(combo.name, manifest.name);
          given.set(`${manifest.id}/${facet.id}/${command.id}`, combo.name);
        }
      }
    }
  }
  return { given, refused };
}

/** The shortcuts the Plugins that `runs` get, handed out again whenever one is enabled or
 * disabled, as the host does when it loads or unloads one. */
export function shortcutsOf(runs: (id: string) => boolean = () => true) {
  return giveShortcuts(
    bundledPlugins.filter(({ manifest }) => runs(manifest.id)).map(({ manifest }) => manifest),
    Object.values(SHORTCUTS).flat(),
  );
}
for (const refused of shortcutsOf().refused) {
  console.warn(`keefast mock: the ${refused.plugin} Plugin's command ${refused.facet}/${refused.command} has no shortcut: ${refused.reason}`);
}

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
    const shortcuts = shortcutsOf(runs);
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
        commands: manifest.capabilities.includes("copy")
          ? (facet.commands ?? []).map((command) => ({
              id: command.id,
              title: command.title,
              field: command.copy,
              shortcut: shortcuts.given.get(`${manifest.id}/${facet.id}/${command.id}`) ?? null,
            }))
          : [],
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

/** The Entry list's subtitle for an Entry with these custom fields and no username, as the
 * host's `list_label`: the label of its first facet, of a Plugin that `runs`, that declares
 * `subtitle`. A username wins,
 * so the list doesn't ask with one. */
export async function listLabel(fields: readonly MockField[], runs: (id: string) => boolean = () => true): Promise<string | null> {
  const indexOf = (name: string) => fields.findIndex((field) => field.name === name);
  for (const { manifest } of bundledPlugins) {
    if (!runs(manifest.id)) continue;
    const readProtected = manifest.capabilities.includes("readProtected");
    for (const facet of manifest.contributions.facets) {
      if (!facet.subtitle || !facet.detect.fields.every((name) => indexOf(name) >= 0)) continue;
      const inputs = facet.fields.map((owned) => {
        const kept = fields[indexOf(owned.name)];
        return { key: owned.key, value: kept && (!kept.protected || readProtected) ? kept.value : undefined };
      });
      try {
        const label = (await hooksOf(manifest.id)).derive(facet.id, inputs).label;
        if (label !== undefined && shorten(label) !== "") return shorten(label);
      } catch (error) {
        console.warn(`keefast mock: the ${manifest.name} Plugin failed`, error);
      }
    }
  }
  return null;
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
