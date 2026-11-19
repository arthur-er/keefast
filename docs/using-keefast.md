# Using keefast

## Keyboard

Ctrl+K (or Ctrl+Shift+P) opens the command palette: type part of a command's name, pick one
with Up/Down and press Enter to run it, or Escape to close the palette. It lists every action of
what has focus, with its keys, and searches Entries as you type.

In the Entry list, Up/Down (Home/End) select an Entry and Enter opens it for editing.
Ctrl+B copies the selected Entry's username, Ctrl+C its password and Ctrl+T its TOTP code.
keefast clears what it copied from the clipboard after 30 seconds, unless you have copied something else since;
change the wait in the settings.

## Layout

The sidebar starts with the Vault you are looking at (click its name to switch), its lists (All
Entries, With TOTP, Recently modified, Expired) and Vault settings, then its Groups, tags and
colours. Below them are the other open and locked Vaults, and the Recycle Bin at the bottom. The
Entry list's search box (`/`) is beside its sort and `+` (a new Entry or Group) buttons, and the
selected Entry fills the rest of the window. The footer has *Open / New* to open another Vault,
and the help (the command palette), settings and lock-all buttons.

## Settings

The settings (Ctrl+, or the gear in the footer and "Settings" in the unlock dialog) have a menu
of their sections and the open Vaults. **General** holds the preferences kept on this machine, in
`keefast/config.toml` in the OS config directory: the theme (Dark, Light, or System, following
the OS's light/dark setting; KeeWeb's themes from earlier versions read as the nearer), the idle lock
and locking with the OS, how long copied secrets stay on the clipboard, autosave, the password
generator, browser integration, favicon download and updates. Each Vault's page (also reached by clicking
it in the footer) saves or locks it, changes its master key, and holds its Default access policy
and Pairings. Tab moves between the settings and Enter or Space picks one; Escape closes them.

Favicon download is off by default, as keefast makes no network requests you haven't agreed to.
Turned on, keefast asks each Entry's site for `/favicon.ico`, or the icon its home page links
to, and never a third-party favicon service. Favicons are kept in memory only, and forgotten when
every Vault locks: a cache on disk would list your sites in the clear.

## Tags, colours, icons and expiry

Edit an Entry to give it tags (separated by commas), a colour, an icon and an expiry date. The
sidebar lists every tag and colour; click one to list the Entries with it across the open
Vaults. Expired Entries are struck through and flagged in the Entry list. Icons are KeePass's
standard icons, drawn with [Lucide](https://lucide.dev) icons (ISC), or PNG images imported
into the Vault as custom
icons. All of these are stored as
KeePassXC, KeePass and KeeWeb store them, so those clients show them too.
