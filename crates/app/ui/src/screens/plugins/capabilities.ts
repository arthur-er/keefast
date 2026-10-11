// What a Plugin's capabilities mean, in plain words: shown before installing it, and in
// Settings → Plugins. Its Owned fields come first: they are a Plugin capability too (ADR 0009).

import type { PluginCapability, PluginFormKind } from "@/bindings";

export type CapabilityKind = "ownedFields" | PluginCapability;

export interface CapabilityLine {
  kind: CapabilityKind;
  title: string;
  /** What an installed Plugin does, in a few words: "Reads its protected fields". */
  short: string;
  desc: string;
  /** Asked for now, by an update, and not agreed to before. */
  isNew: boolean;
}

/** "IBAN and BIC", "IBAN, BIC and Account holder". */
export function listNames(names: readonly string[]): string {
  const quoted = names.map((name) => `“${name}”`);
  if (quoted.length <= 1) return quoted.join("");
  return `${quoted.slice(0, -1).join(", ")} and ${quoted[quoted.length - 1]}`;
}

/** What a Plugin may do, as installed or as a package asks. */
interface Asked {
  capabilities: readonly PluginCapability[];
  ownedFields: readonly string[];
  /** The Owned fields the Companion extension may fill into web forms. */
  filledFields?: readonly string[];
  fillForms?: readonly PluginFormKind[];
}

/** The lines saying what a Plugin may do. `fresh` lists what an update asks for that wasn't agreed
 * to before; without it nothing is new. */
export function capabilityLines(
  plugin: Asked,
  fresh?: { capabilities: readonly PluginCapability[]; ownedFields: readonly string[]; filledFields?: readonly string[] },
): CapabilityLine[] {
  const lines: CapabilityLine[] = [];
  if (plugin.ownedFields.length > 0) {
    const added = fresh?.ownedFields ?? [];
    lines.push({
      kind: "ownedFields",
      title: plugin.ownedFields.length === 1 ? "Keep its data in one field of your Entries" : `Keep its data in ${plugin.ownedFields.length} fields of your Entries`,
      short: plugin.ownedFields.join(", "),
      desc: `${listNames(plugin.ownedFields)}. They stay ordinary custom fields, readable in any KeePass app.`,
      isNew: added.length > 0,
    });
  }
  for (const capability of CAPABILITY_ORDER) {
    if (!plugin.capabilities.includes(capability)) continue;
    const isNew = fresh?.capabilities.includes(capability) ?? false;
    if (capability === "browserFill") {
      // An update filling one more field asks again, though the capability isn't new.
      lines.push({ kind: capability, ...browserFill(plugin), isNew: isNew || (fresh?.filledFields?.length ?? 0) > 0 });
    } else {
      lines.push({ kind: capability, ...CAPABILITIES[capability], isNew });
    }
  }
  return lines;
}

const CAPABILITY_ORDER: readonly PluginCapability[] = ["readProtected", "copy", "browserFill"];

const FORMS: Record<PluginFormKind, string> = { payment: "payment forms" };

/** Browser fill in plain words: which forms, with which fields. */
function browserFill({ filledFields = [], fillForms = [] }: Asked): { title: string; short: string; desc: string } {
  const forms = fillForms.length === 1 ? FORMS[fillForms[0]!] : "forms";
  return {
    title: `Fill ${forms} in your browser`,
    short: `Fills ${forms} in your browser`,
    desc:
      `With ${filledFields.length > 0 ? listNames(filledFields) : "its fields"}, when you pick the Entry over a page. ` +
      "Only into the fields made for them, and never on a site you turned the Entry down for.",
  };
}

const CAPABILITIES: Record<Exclude<PluginCapability, "browserFill">, { title: string; short: string; desc: string }> = {
  readProtected: {
    title: "Read its protected fields",
    short: "Reads its protected fields",
    desc: "To show a short label in their place, such as “Visa •••• 1111”. They stay hidden until you show them.",
  },
  copy: {
    title: "Let you copy its fields",
    short: "Lets you copy its fields",
    desc: "keefast copies them, and clears the clipboard as it does for passwords.",
  },
};
