// Tag colours (Vault browser C, Entry editor A): each tag is drawn in the colour chosen for it in
// its Vault, kept in the Vault's custom data, or else in one picked from its name, so every tag
// has its own colour from the start and keeps it.

import { useCallback } from "react";

import type { Color, VaultId, VaultSummary } from "@/bindings";
import { useWorkspace } from "@/api/queries";

/** The colours a tag's name picks from, in the prototype's order. */
const AUTOMATIC: readonly Color[] = ["violet", "blue", "green", "orange", "red", "yellow"];

/** The colour a tag without a chosen one is drawn in: the same for the same name, everywhere. */
export function automaticTagColor(tag: string): Color {
  let sum = 0;
  for (const char of tag) sum += char.codePointAt(0)!;
  return AUTOMATIC[sum % AUTOMATIC.length]!;
}

/** The colour chosen for the tag in the Vault, if one was. */
export function chosenTagColor(vault: VaultSummary | undefined, tag: string): Color | null {
  return vault?.tagColors.find((chosen) => chosen.tag === tag)?.color ?? null;
}

/** The colour the tag is drawn in, in the Vault. */
export function tagColor(vault: VaultSummary | undefined, tag: string): Color {
  return chosenTagColor(vault, tag) ?? automaticTagColor(tag);
}

/** The colour of a tag in a Vault, from the latest snapshot. */
export function useTagColor(): (vault: VaultId, tag: string) => Color {
  const vaults = useWorkspace().data?.vaults;
  return useCallback(
    (vault: VaultId, tag: string) => tagColor(vaults?.find((summary) => summary.id === vault), tag),
    [vaults],
  );
}
