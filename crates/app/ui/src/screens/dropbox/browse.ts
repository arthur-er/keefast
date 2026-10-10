// What Open from Dropbox A shows about folders and files: the path as crumbs, a file's size and
// when it changed, and whether it is a Vault file.

import type { DropboxEntry } from "@/bindings";
import { formatDate } from "@/lib/format";

/** The crumbs to `folder` (`""` is the top of the Dropbox): each with the folder it opens. */
export function crumbs(folder: string): { path: string; name: string }[] {
  const names = folder.split("/").filter(Boolean);
  return [{ path: "", name: "Dropbox" }, ...names.map((name, index) => ({ path: `/${names.slice(0, index + 1).join("/")}`, name }))];
}

/** The folder holding `path`, or `null` at the top of the Dropbox. */
export function parentFolder(path: string): string | null {
  if (!path) return null;
  return path.slice(0, path.lastIndexOf("/"));
}

export function isVaultFile(entry: Pick<DropboxEntry, "name" | "isFolder">): boolean {
  return !entry.isFolder && /\.kdbx$/i.test(entry.name);
}

/** "Today, 09:12", "Yesterday, 21:40", else the date as the app writes dates. */
export function modifiedLabel(seconds: number, now = Date.now() / 1000): string {
  const date = new Date(seconds * 1000);
  const time = date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  const day = (at: Date) => new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime();
  const days = Math.round((day(new Date(now * 1000)) - day(date)) / 86_400_000);
  if (days === 0) return `Today, ${time}`;
  if (days === 1) return `Yesterday, ${time}`;
  return formatDate(seconds, now);
}

/** "57 KB", "1.2 MB", "411 B". */
export function sizeLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
