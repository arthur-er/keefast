# Browser integration

keefast speaks the KeePassXC-Browser protocol (ADR 0003) with its Companion extension, and the
stock KeePassXC-Browser extension works with it too. Turn on browser integration in keefast's settings:
it registers `keefast-native-host`, installed next to the `keefast` binary, with Chrome,
Chromium, Brave, Edge and Vivaldi. It registers under KeePassXC's host name, so it takes over
from a KeePassXC registered for the same browsers; turning it off removes only keefast's
registrations. Then click "Connect" in the extension and name the browser in keefast to pair it
with the Vault you are looking at.

To try the Companion extension, build it with [WXT](https://wxt.dev) and load
`extension/.output/chrome-mv3` as an unpacked extension in `chrome://extensions` (with
"Developer mode" on). Each pull request's CI run also has it as a zip.

```sh
cd extension
pnpm install
pnpm test
pnpm build   # or `pnpm dev` for a browser that reloads it as you edit
```

Click its toolbar button, then "Connect", and name the browser in keefast. On a login page, the
popup lists the site's Entries: click one to fill it, or press Alt+Shift+K to fill the only one.
The extension never asks for a master password: when keefast is locked or not running, it says
so, and you unlock keefast itself.

Turn on "Show keefast in sign-in fields" in the popup to get keefast's icon in login, 2FA and
new password fields, as KeePassXC-Browser shows its own. Chrome then asks to let the extension
read every site, since it has to look for their sign-in fields as they load (ADR 0007).
Clicking the icon fills the site's only Entry, or opens a command palette over the page: it
lists the site's Entries, and as you type, searches keefast for any other, which keefast then
remembers as allowed for the site (and, with "Add the site to Entries I fill from search", adds
the site to). In a new password field, the palette shows a password from keefast's generator to
fill. "Don't show keefast on this site" in the palette hides the icon there. With the icons
off, nothing is filled until you ask, and pages are read only when you do.

On a payment form, the icon sits in the card fields, and opens the palette listing the Entries
with a Card (the Card Plugin fills payment forms, a Plugin capability you can see in Settings →
Plugins). Picking one fills the card's number, cardholder, expiry and CVV, the CVV only into a
field the page marked as a security code. Payment fields in a payment provider's frame get the
icon too; the palette then names the frame's site, and fills only there and in the page itself
(ADR 0010). The shortcut opens the same palette on a payment form. A Card is never filled
without you picking it.

Once you've filled a page, the extension also notices the logins you submit there: a sign-in
it doesn't have, a sign-up, or a password change. keefast asks you whether to save it, and
where; a changed password updates its Entry. A sign-in with an Entry as filled sends nothing.

To try the protocol without a browser, run keefast with browser integration on and:

```sh
cargo run -p keefast-native-host --example fake_extension -- target/debug/keefast-native-host
```
