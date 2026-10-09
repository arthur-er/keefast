// What the extension keeps in its storage, which only its own pages and service worker read:
// pages' scripts never do (ADR 0007). It holds no secrets (ADR 0001).
import { storage } from "wxt/utils/storage";
import type { Pairing } from "./keefast";

/** The extension's Pairings with Vaults. */
export const pairings = storage.defineItem<Pairing[]>("local:pairings", { fallback: [] });

/** The origins whose sign-in fields the user asked keefast not to show its icon in. */
export const hiddenSites = storage.defineItem<string[]>("local:hiddenSites", { fallback: [] });

/** Whether filling an Entry found by the palette's search adds the site to the Entry's URLs. */
export const addSiteOnPick = storage.defineItem<boolean>("local:addSiteOnPick", {
  fallback: false,
});

/**
 * The permission keefast's icons in sign-in fields need: their script runs in every page as it
 * loads, so it is asked for only when the user turns them on.
 */
export const ALL_SITES = { origins: ["<all_urls>"] };

/** The script that shows keefast's icons in sign-in fields, as registered with the browser. */
export const FIELDS_SCRIPT = { id: "fields", file: "/fields.js" } as const;
