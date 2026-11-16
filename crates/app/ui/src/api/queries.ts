// Queries over the backend with TanStack Query. Everything the backend can change is refetched
// when it emits `workspaceChanged` (see `useBackendEvents`), so screens just read these hooks.

import { useEffect } from "react";
import { QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";

import type { EntryId, EntryQuery, KdfChoice, Scope, VaultId } from "@/bindings";

import { api, onEvent } from "./index";

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // Fresh until the backend says otherwise.
        staleTime: Infinity,
        refetchOnWindowFocus: false,
        retry: false,
      },
    },
  });
}

export const keys = {
  workspace: ["workspace"] as const,
  launch: ["launch"] as const,
  settings: ["settings"] as const,
  startOnBoot: ["startOnBoot"] as const,
  entries: (query: EntryQuery) => ["entries", query] as const,
  palette: (search: string, names: string[]) => ["palette", search, names] as const,
  entry: (vault: VaultId, entry: EntryId) => ["entry", vault, entry] as const,
  groups: (vault: VaultId) => ["groups", vault] as const,
  history: (vault: VaultId, entry: EntryId) => ["history", vault, entry] as const,
  totp: (vault: VaultId, entry: EntryId) => ["totp", vault, entry] as const,
  customIcons: (vault: VaultId) => ["customIcons", vault] as const,
  vaultSettings: (vault: VaultId) => ["vaultSettings", vault] as const,
  groupForNewItems: (scope: Scope) => ["groupForNewItems", scope] as const,
  generatorLimits: ["generatorLimits"] as const,
  unlockTime: (kdf: KdfChoice) => ["unlockTime", kdf] as const,
  dropbox: ["dropbox"] as const,
  plugins: ["plugins"] as const,
  facets: ["facets"] as const,
};

/** Refetches every query whenever the backend's Workspace changes. Mount once, at the root. */
export function useBackendEvents() {
  const client = useQueryClient();
  useEffect(
    () =>
      onEvent("workspaceChanged", () => {
        // Queries that only read once (launch args, limits, timings) are left alone.
        void client.invalidateQueries({
          predicate: (query) => !["launch", "generatorLimits", "unlockTime"].includes(query.queryKey[0] as string),
        });
      }),
    [client],
  );
}

export function useWorkspace() {
  return useQuery({ queryKey: keys.workspace, queryFn: api.getWorkspace });
}

/** The facets Plugins contribute, which Entry editor A can add to an Entry. */
export function useFacets() {
  return useQuery({ queryKey: keys.facets, queryFn: api.listFacets });
}

export function useLaunch() {
  return useQuery({ queryKey: keys.launch, queryFn: api.getLaunch });
}

export function useSettings() {
  return useQuery({ queryKey: keys.settings, queryFn: api.getSettings });
}

/** Whether keefast is registered to start when the user logs in, as the OS has it. Read again
 * each time it is shown, as the user may have removed the entry by hand meanwhile. */
export function useStartOnBoot() {
  return useQuery({ queryKey: keys.startOnBoot, queryFn: api.getStartOnBoot, refetchOnMount: "always" });
}

export function usePlugins() {
  return useQuery({ queryKey: keys.plugins, queryFn: api.listPlugins });
}

export function useEntries(query: EntryQuery) {
  return useQuery({
    queryKey: keys.entries(query),
    queryFn: () => api.listEntries(query),
    placeholderData: (previous) => previous,
  });
}

/** The command palette's search while one is typed: how each of `names` (its commands' titles)
 * matches, by name, and the Entries found. The last search's matches show until the next's come. */
export function usePaletteSearch(search: string, names: string[]) {
  return useQuery({
    queryKey: keys.palette(search, names),
    queryFn: async () => {
      const found = await api.searchPalette(search, names);
      return { names: new Map(names.map((name, index) => [name, found.names[index] ?? null])), entries: found.entries };
    },
    enabled: search.trim() !== "",
    placeholderData: (previous) => previous,
  });
}

export function useEntry(vault: VaultId | undefined, entry: EntryId | undefined) {
  return useQuery({
    queryKey: keys.entry(vault ?? -1, entry ?? ""),
    queryFn: () => api.getEntry(vault!, entry!),
    enabled: vault !== undefined && entry !== undefined,
  });
}

export function useGroups(vault: VaultId | undefined) {
  return useQuery({
    queryKey: keys.groups(vault ?? -1),
    queryFn: () => api.listGroups(vault!),
    enabled: vault !== undefined,
  });
}

export function useEntryHistory(vault: VaultId | undefined, entry: EntryId | undefined) {
  return useQuery({
    queryKey: keys.history(vault ?? -1, entry ?? ""),
    queryFn: () => api.getEntryHistory(vault!, entry!),
    enabled: vault !== undefined && entry !== undefined,
  });
}

/** The Entry's TOTP code, refetched as each code runs out, and as the window shows again: while
 * it is hidden (minimised, on another workspace) the refetch interval waits, so a code that ran
 * out meanwhile would show at "0 s" until the next one. */
export function useTotpCode(vault: VaultId | undefined, entry: EntryId | undefined, enabled = true) {
  return useQuery({
    queryKey: keys.totp(vault ?? -1, entry ?? ""),
    queryFn: () => api.getTotpCode(vault!, entry!),
    enabled: enabled && vault !== undefined && entry !== undefined,
    staleTime: 0,
    refetchInterval: (query) => (query.state.data ? query.state.data.remaining * 1000 : false),
    refetchOnWindowFocus: true,
  });
}

export function useVaultSettings(vault: VaultId | undefined) {
  return useQuery({
    queryKey: keys.vaultSettings(vault ?? -1),
    queryFn: () => api.getVaultSettings(vault!),
    enabled: vault !== undefined,
  });
}

export function useCustomIcons(vault: VaultId | undefined) {
  return useQuery({
    queryKey: keys.customIcons(vault ?? -1),
    queryFn: () => api.listCustomIcons(vault!),
    enabled: vault !== undefined,
  });
}
