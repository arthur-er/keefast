// Plugins' commands (ADR 0009), such as the Card's "Copy card number": declared in a facet's
// manifest, listed in the command palette's Entry section for the selected Entry when it has the
// facet, and run by the shortcut the host let the Plugin have. Each copies an Owned field by its
// index, Rust-to-clipboard through `copyField`, as the section's copy buttons do.

import type { EntryDetail, FacetCommandView, FacetView, OwnedFieldView } from "@/bindings";
import { type AppCommand, type AppCommandId, useAppCommand } from "@/shell/commands";
import type { CopyHandler } from "@/screens/vault-browser/FieldRow";

import { facetIcon } from "./FacetSection";

/** One of an Entry's facets' commands, with the Owned field it copies. */
export interface FacetCopy {
  facet: FacetView;
  command: FacetCommandView;
  field: OwnedFieldView;
  /** Unique among the app's commands: `plugin:keefast.card:card:copy-number`. */
  id: AppCommandId;
  /** Whether the Entry has a value to copy. */
  enabled: boolean;
  /** The key the detail's "Copied" flash goes by, as `FacetSection`'s copy button's. */
  flashKey: string;
}

/** The commands of an Entry's facets, in the order the Plugins and their manifests give them. */
export function facetCopies(entry: EntryDetail | undefined): FacetCopy[] {
  return (entry?.facets ?? []).flatMap((facet) =>
    facet.commands.flatMap((command) => {
      const field = facet.fields.find((candidate) => candidate.key === command.field);
      if (!field) return [];
      return [
        {
          facet,
          command,
          field,
          id: `plugin:${facet.plugin}:${facet.facet}:${command.id}` as const,
          enabled: field.index !== null && !field.isEmpty,
          flashKey: `facet:${facet.plugin}:${facet.facet}:${field.key}`,
        },
      ];
    }),
  );
}

/** Registers the selected Entry's Plugin commands while it is shown. */
export function FacetCommands({ entry, active, onCopy }: { entry: EntryDetail; active: boolean; onCopy: CopyHandler }) {
  return facetCopies(entry).map((copy) => <FacetCommand key={copy.id} copy={copy} active={active} onCopy={onCopy} />);
}

function FacetCommand({ copy, active, onCopy }: { copy: FacetCopy; active: boolean; onCopy: CopyHandler }) {
  const { facet, command, field } = copy;
  const registered: AppCommand = {
    id: copy.id,
    title: command.title,
    section: "Entry",
    icon: facetIcon(facet.icon),
    shortcut: command.shortcut !== null ? [command.shortcut] : undefined,
    // A Plugin's keys never take a key from a field being typed in.
    inFields: false,
    enabled: copy.enabled,
    run: () => {
      if (field.index !== null) void onCopy({ kind: "customField", name: field.name }, field.name, copy.flashKey);
    },
  };
  useAppCommand(registered, active);
  return null;
}
