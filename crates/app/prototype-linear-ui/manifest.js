/* PROTOTYPE — every screen, its variants, and the states worth a screenshot.
 * Used by index.html (gallery) and shoot.sh (regenerates shots/). */
var MANIFEST = [
  { key: "unlock", title: "Open a Vault", variants: {
    A: ["Focused card, recents below", [["", "Default"], ["state=error", "Wrong master key"]]],
    B: ["Vault files rail + unlock pane", [["", "Default"], ["state=new", "New Vault"]]],
    C: ["Launcher", [["", "Pick a Vault"], ["state=password", "Password inline"]]],
  } },
  { key: "vault-browser", title: "Vault browser", variants: {
    A: ["Three-pane with property rail", [["", "Default"], ["state=menu", "Entry context menu"]]],
    B: ["Dense table + peek panel", [["", "Peek open"], ["state=nopeek", "Table only"]]],
    C: ["Vault switcher, grouped list, tabbed detail", [["", "Default"], ["state=locked", "Locked Vault"]]],
  } },
  { key: "entry-editor", title: "Entry editor", variants: {
    A: ["Edit in place", [["", "Default"], ["state=icon", "Icon picker"]]],
    B: ["Side sheet", [["", "Default"], ["state=error", "Validation"]]],
    C: ["Focused editor + properties column", [["", "Default"], ["state=error", "Validation"]]],
  } },
  { key: "password-generator", title: "Password generator", variants: {
    A: ["Popover on the field", [["", "Characters"], ["state=error", "No classes"]]],
    B: ["Inline drawer", [["", "Default"]]],
    C: ["Palette of candidates", [["", "Default"]]],
  } },
  { key: "totp", title: "TOTP", variants: {
    A: ["Code card + setup dialog", [["", "Code"], ["state=setup", "Set up TOTP"]]],
    B: ["TOTP as a field, inline setup", [["", "Code"], ["state=error", "Inline setup, error"]]],
  } },
  { key: "entry-history", title: "Entry history", variants: {
    A: ["Side sheet timeline", [["", "Timeline"], ["state=empty", "Empty"]]],
    B: ["Full-window split", [["", "Versions | diff"], ["state=compare", "Compare with current"]]],
  } },
  { key: "command-palette", title: "Command palette", variants: {
    A: ["Classic centred (Linear)", [["", "Default"], ["q=gh", "Query “gh”"]]],
    B: ["Raycast-style with preview", [["", "Default"], ["state=actions", "Actions menu"]]],
    C: ["Scoped to the Entry", [["", "Default"], ["state=move", "Move to Group"]]],
  } },
  { key: "settings", title: "General Settings", variants: {
    A: ["Full window, settings nav", [["", "General"], ["page=browser&state=browser-error", "Browser error"]]],
    B: ["Centred modal with tabs", [["", "Default"], ["tab=generator", "Generator tab"]]],
    C: ["One long page + index", [["", "Default"], ["state=search", "Search “lock”"]]],
  } },
  { key: "vault-settings", title: "Vault settings & Master key", variants: {
    A: ["Settings page, inline change-key card", [["", "Default"], ["state=key", "Change master key"]]],
    B: ["Dialog, stepped change-key", [["", "Default"], ["state=key", "Change master key"]]],
  } },
  { key: "dropbox", title: "Open from Dropbox", variants: {
    A: ["File browser dialog", [["", "Browse"], ["state=connect", "Connect"], ["state=sync", "Sync status"]]],
    B: ["Stepped panel in Open a Vault", [["", "Browse"], ["state=connect", "Connect"]]],
  } },
  { key: "dialogs", title: "Browser requests & confirms", variants: {
    A: ["Centred dialogs", [["state=access", "Allow access?"], ["state=pair", "Pair a browser"], ["state=save-login", "Save login?"], ["state=lock-unsaved", "Lock, unsaved"], ["state=delete", "Delete for good"]]],
    B: ["Request cards + anchored confirms", [["state=access", "Allow access?"], ["state=save-login", "Save login?"], ["state=delete", "Delete popover"]]],
    C: ["Requests inbox panel", [["state=access", "Allow access?"], ["state=save-login", "Save login?"], ["state=delete", "Inline delete"]]],
  } },
];
/** shots/<screen>-<variant>[-<slug>].png for a state query. */
function shotName(screen, v, q) {
  if (!q) return `${screen}-${v}.png`;
  const slug = q.split("&").map((p) => p.split("=")[1]).join("-");
  return `${screen}-${v}-${slug}.png`;
}
if (typeof module !== "undefined") module.exports = { MANIFEST, shotName };
