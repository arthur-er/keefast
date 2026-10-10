// What KeePass's 69 standard icons are called (KeePass's PwIcon), in words, by number: the icon
// picker filters on them and shows them as tooltips. The drawings are the kit's
// `STANDARD_ICONS`.

export const STANDARD_ICON_NAMES: readonly string[] = [
  "Key", "World", "Warning", "Network server", "Marked directory", "User communication", "Parts", "Notepad",
  "World socket", "Identity", "Paper ready", "Digicam", "IR communication", "Multiple keys", "Energy", "Scanner",
  "World star", "CD-ROM", "Monitor", "Email", "Configuration", "Clipboard ready", "New paper", "Screen",
  "Energy careful", "Email box", "Disk", "Drive", "Paper question", "Terminal encrypted", "Console", "Printer",
  "Program icons", "Run", "Settings", "World computer", "Archive", "Home banking", "Windows drive", "Clock",
  "Email search", "Paper flag", "Memory", "Trash bin", "Note", "Expired", "Info", "Package", "Folder",
  "Open folder", "Package folder", "Open lock", "Locked paper", "Checked", "Pen", "Thumbnail", "Book", "List",
  "User key", "Tool", "Home", "Star", "Tux", "Feather", "Apple", "Wiki", "Money", "Certificate", "Phone",
];

/** The standard icons whose names match `filter`, by number. */
export function filterStandardIcons(filter: string): number[] {
  const needle = filter.trim().toLowerCase();
  return STANDARD_ICON_NAMES.flatMap((name, number) => (!needle || name.toLowerCase().includes(needle) ? [number] : []));
}
