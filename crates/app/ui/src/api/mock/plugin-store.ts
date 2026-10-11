// The mock's installed Plugins: Settings → Plugins and installing from a package, as
// crates/plugins/src/store.rs does them, over made-up packages. keefast's own Plugins are the ones
// compiled in (`bundledPlugins`); installed ones have no component in the mock, so they find no
// facets.
//
// The file dialog "picks" the package the scene sets (`__keefastMock.simulate("plugin-…")`):
//   plugin-new (the default)  IBAN 1.0.0 by Acme Ltd, a publisher keefast hasn't seen
//   plugin-update             IBAN installed; the package is 2.0.0, asking for more
//   plugin-clash              a Plugin owning `Card number`, which keefast's Card owns
//   plugin-key-changed        IBAN installed; the package is signed by another key
//   plugin-malformed          a package changed after it was signed
//   plugin-keefast            Wi-Fi network 1.0.0, signed by keefast's release key
//   plugin-cancel             the dialog is cancelled
//   plugins-installed         IBAN installed, Passport installed but disabled, and a Plugin whose
//                             package failed its checks; the package is IBAN 2.0.0. IBAN's
//                             "Copy IBAN" asks for Ctrl Shift C, which it gets only while the Card
//                             is disabled
//   plugins-dev-build         keefast's Card signed by the key a development build made for itself,
//                             not keefast's release key

import { bundledPlugins } from "virtual:keefast/bundled-plugins";

import { shortcutsOf } from "./plugins";

import type { PluginCapability, PluginList, PluginOffer, PluginPublisher, PluginView } from "@/bindings";

export type PluginScene =
  | "plugin-new"
  | "plugin-update"
  | "plugin-clash"
  | "plugin-key-changed"
  | "plugin-malformed"
  | "plugin-keefast"
  | "plugin-cancel"
  | "plugins-installed"
  | "plugins-dev-build";

export const PLUGIN_SCENES: readonly PluginScene[] = [
  "plugin-new",
  "plugin-update",
  "plugin-clash",
  "plugin-key-changed",
  "plugin-malformed",
  "plugin-keefast",
  "plugin-cancel",
  "plugins-installed",
  "plugins-dev-build",
];

/** A refusal, as the app's `ApiError`. */
export class PluginRefusal extends Error {
  constructor(
    message: string,
    readonly detail: string | null = null,
  ) {
    super(message);
  }
}

/** A made-up package: its manifest's facts, and the key it is signed by. */
interface FakePackage {
  path: string;
  id: string;
  name: string;
  version: string;
  publisher: string;
  fingerprint: string;
  /** Signed by keefast's release key. */
  keefast?: boolean;
  capabilities: PluginCapability[];
  ownedFields: string[];
  icon: string | null;
  /** Why the app would refuse it before saying what it asks for. */
  refusal?: PluginRefusal;
}

interface Installed {
  id: string;
  name: string;
  version: string | null;
  publisher: string;
  fingerprint: string;
  keefast?: boolean;
  enabled: boolean;
  capabilities: PluginCapability[];
  ownedFields: string[];
  icon: string | null;
  problem: string | null;
  /** Its commands' titles and the shortcuts they ask for. The fake packages have no component,
   * so these stand in for the manifest's `commands`. */
  shortcuts?: { title: string; shortcut: string }[];
}

/** A Plugin icon: a tinted tile with a letter, as a package's `icon.png` would be. */
function tile(letter: string, color: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="8" fill="${color}"/><text x="16" y="21.5" font-family="Inter,system-ui,sans-serif" font-size="15" font-weight="650" fill="#fff" text-anchor="middle">${letter}</text></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const ACME = "3F2A 91C0 7B1E 44D2";
/** keefast's release key, and the key a development build makes for itself. */
const KEEFAST_KEY = "7C1D 52E8 A06B 93F4";
const DEV_BUILD_KEY = "E25B 0C94 6D17 A83E";
const OTHER_KEY = "D41C 0E7A 22B9 F360";

const IBAN_1: FakePackage = {
  path: "~/Downloads/acme-iban-1.0.0.kfplugin",
  id: "acme.iban",
  name: "IBAN",
  version: "1.0.0",
  publisher: "Acme Ltd",
  fingerprint: ACME,
  capabilities: ["copy"],
  ownedFields: ["IBAN", "BIC"],
  icon: tile("I", "#2f7d6d"),
};

const IBAN_2: FakePackage = {
  ...IBAN_1,
  path: "~/Downloads/acme-iban-2.0.0.kfplugin",
  version: "2.0.0",
  capabilities: ["copy", "readProtected"],
  ownedFields: ["IBAN", "BIC", "Account holder"],
};

const PACKAGES: Record<Exclude<PluginScene, "plugin-cancel" | "plugins-installed" | "plugins-dev-build">, FakePackage> = {
  "plugin-new": IBAN_1,
  "plugin-keefast": {
    path: "~/Downloads/keefast-wifi-1.0.0.kfplugin",
    id: "keefast.wifi",
    name: "Wi-Fi network",
    version: "1.0.0",
    publisher: "keefast",
    fingerprint: KEEFAST_KEY,
    keefast: true,
    capabilities: ["copy"],
    ownedFields: ["SSID", "Wi-Fi password", "Security"],
    icon: tile("W", "#2563eb"),
  },
  "plugin-update": IBAN_2,
  "plugin-clash": {
    path: "~/Downloads/cardvault-pro.kfplugin",
    id: "cardvault.pro",
    name: "CardVault Pro",
    version: "3.2.0",
    publisher: "CardVault Inc.",
    fingerprint: "8B05 6C13 E9A7 0F42",
    capabilities: ["readProtected", "copy"],
    ownedFields: ["Card number", "Expiry date", "Security code"],
    icon: tile("C", "#8a4fd8"),
    refusal: new PluginRefusal(
      "“CardVault Pro” would keep its data in the field “Card number”, which the installed Plugin “Card” owns. Two Plugins can't own the same field.",
    ),
  },
  "plugin-key-changed": {
    ...IBAN_2,
    path: "~/Downloads/iban-2.0.0-mirror.kfplugin",
    fingerprint: OTHER_KEY,
    refusal: new PluginRefusal(
      "“IBAN” is signed by another key than the installed one, so it may not come from the same publisher. If you trust it, remove the installed “IBAN” first.",
    ),
  },
  "plugin-malformed": {
    ...IBAN_1,
    path: "~/Downloads/acme-iban-1.0.0 (1).kfplugin",
    refusal: new PluginRefusal(
      "This file isn't a Plugin package keefast can install.",
      "The package's signature doesn't match its contents: it was changed after it was signed, or signed with another key than its manifest names.",
    ),
  },
};

function installedOf(pkg: FakePackage): Installed {
  return {
    id: pkg.id,
    name: pkg.name,
    version: pkg.version,
    publisher: pkg.publisher,
    fingerprint: pkg.fingerprint,
    keefast: pkg.keefast,
    enabled: true,
    capabilities: [...pkg.capabilities],
    ownedFields: [...pkg.ownedFields],
    icon: pkg.icon,
    problem: null,
  };
}

const PASSPORT: Installed = {
  id: "travelkit.passport",
  name: "Passport",
  version: "0.4.1",
  publisher: "TravelKit",
  fingerprint: "61E0 3B7D A18C 5F29",
  enabled: false,
  capabilities: ["readProtected"],
  ownedFields: ["Passport number", "Nationality", "Issued", "Expires"],
  icon: tile("P", "#c2410c"),
  problem: null,
};

const BROKEN: Installed = {
  id: "example.loyalty",
  name: "example.loyalty",
  version: null,
  publisher: "",
  fingerprint: "0A9F 4E21 77C3 B806",
  enabled: true,
  capabilities: ["copy"],
  ownedFields: ["Member number"],
  icon: null,
  problem: "the package's signature doesn't match its contents: it was changed after it was signed, or signed with another key than its manifest names",
};

export function createMockPluginStore(changed: () => void) {
  let installed: Installed[] = [];
  let disabledBundled = new Set<string>();
  let next: PluginScene = "plugin-new";
  let devBuild = false;

  const publisherOf = (plugin: { publisher: string; fingerprint: string; keefast?: boolean }): PluginPublisher =>
    plugin.keefast ? { kind: "keefast", fingerprint: plugin.fingerprint, release: true } : { kind: "unverified", name: plugin.publisher, fingerprint: plugin.fingerprint };

  function view(plugin: Installed): PluginView {
    return {
      id: plugin.id,
      name: plugin.name,
      version: plugin.version,
      publisher: publisherOf(plugin),
      bundled: false,
      enabled: plugin.enabled,
      running: plugin.enabled && plugin.problem === null,
      capabilities: plugin.capabilities,
      ownedFields: plugin.ownedFields,
      icon: plugin.icon,
      problem: plugin.enabled ? plugin.problem : null,
      refusedShortcuts: plugin.enabled ? refusedOf(plugin) : [],
    };
  }

  /** Why an installed Plugin's commands lack the shortcuts they ask for, as the host says: the
   * running bundled Plugins loaded first. */
  function refusedOf(plugin: Installed): string[] {
    const runs = (id: string) => !disabledBundled.has(id);
    const given = shortcutsOf(runs).given;
    const owner = (shortcut: string) =>
      [...given.entries()].find(([, taken]) => taken === shortcut)?.[0].split("/")[0];
    return (plugin.shortcuts ?? []).flatMap(({ title, shortcut }) => {
      const id = owner(shortcut);
      const name = bundledPlugins.find(({ manifest }) => manifest.id === id)?.manifest.name;
      return name ? [`${title} has no shortcut: ${shortcut} is taken by the ${name} Plugin.`] : [];
    });
  }

  function list(): PluginList {
    const bundled = bundledPlugins.map(({ manifest }): PluginView => {
      const enabled = !disabledBundled.has(manifest.id);
      return {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        publisher: { kind: "keefast", fingerprint: devBuild ? DEV_BUILD_KEY : KEEFAST_KEY, release: !devBuild },
        bundled: true,
        enabled,
        running: enabled,
        capabilities: manifest.capabilities,
        ownedFields: manifest.contributions.facets.flatMap((facet) => facet.fields.map((field) => field.name)),
        icon: null,
        problem: null,
        refusedShortcuts: shortcutsOf((id) => !disabledBundled.has(id))
          .refused.filter((refused) => refused.plugin === manifest.id)
          .map((refused) => {
            const command = manifest.contributions.facets.find((facet) => facet.id === refused.facet)?.commands?.find((c) => c.id === refused.command);
            return `${command?.title ?? refused.command} has no shortcut: ${refused.reason}.`;
          }),
      };
    });
    const others = [...installed].sort((a, b) => a.name.localeCompare(b.name)).map(view);
    return { plugins: [...bundled, ...others], error: null };
  }

  /** What installing the scene's package would do, or why it is refused. */
  function choose(): PluginOffer | null {
    if (next === "plugin-cancel") return null;
    const pkg = next === "plugins-installed" || next === "plugins-dev-build" ? IBAN_2 : PACKAGES[next];
    if (pkg.refusal) throw pkg.refusal;
    const agreed = installed.find((plugin) => plugin.id === pkg.id);
    return {
      path: pkg.path,
      digest: `${pkg.id}@${pkg.version}`,
      id: pkg.id,
      name: pkg.name,
      version: pkg.version,
      publisher: publisherOf(pkg),
      newKey: !agreed && !pkg.keefast,
      installedVersion: agreed?.version ?? null,
      capabilities: pkg.capabilities,
      ownedFields: pkg.ownedFields,
      newCapabilities: pkg.capabilities.filter((capability) => !agreed?.capabilities.includes(capability)),
      newOwnedFields: pkg.ownedFields.filter((field) => !agreed?.ownedFields.includes(field)),
      icon: pkg.icon,
    };
  }

  function install(path: string, digest: string): null {
    const pkg = Object.values(PACKAGES).find((candidate) => candidate.path === path && !candidate.refusal);
    if (!pkg || `${pkg.id}@${pkg.version}` !== digest) {
      throw new PluginRefusal("The package changed since keefast showed what it asks for. Choose it again to see what it asks for now.");
    }
    installed = [...installed.filter((plugin) => plugin.id !== pkg.id), installedOf(pkg)];
    changed();
    return null;
  }

  function setEnabled(id: string, enabled: boolean): null {
    if (bundledPlugins.some(({ manifest }) => manifest.id === id)) {
      disabledBundled = new Set(disabledBundled);
      if (enabled) disabledBundled.delete(id);
      else disabledBundled.add(id);
    } else {
      const plugin = installed.find((candidate) => candidate.id === id);
      if (!plugin) throw new PluginRefusal("That Plugin is no longer installed.");
      plugin.enabled = enabled;
    }
    changed();
    return null;
  }

  function remove(id: string): null {
    const bundled = bundledPlugins.find(({ manifest }) => manifest.id === id);
    if (bundled) throw new PluginRefusal(`${bundled.manifest.name} comes with keefast and can't be removed. Disable it instead.`);
    installed = installed.filter((plugin) => plugin.id !== id);
    changed();
    return null;
  }

  function simulate(scene: PluginScene) {
    next = scene;
    devBuild = scene === "plugins-dev-build";
    const iban = installedOf(IBAN_1);
    if (scene === "plugin-update" || scene === "plugin-key-changed") {
      installed = [...installed.filter((plugin) => plugin.id !== iban.id), iban];
    }
    if (scene === "plugins-installed") {
      installed = [{ ...iban, shortcuts: [{ title: "Copy IBAN", shortcut: "Ctrl Shift C" }] }, { ...PASSPORT }, { ...BROKEN }];
    }
  }

  return {
    list,
    choose,
    install,
    setEnabled,
    remove,
    simulate,
    /** Whether a Plugin compiled in runs: the mock finds its facets only then. */
    runs: (id: string) => !disabledBundled.has(id),
  };
}
