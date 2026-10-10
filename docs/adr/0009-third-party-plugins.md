# Third-party Plugins: WASM in the Rust process, declarative UI, data in plain custom fields

keefast gains Plugins: packages that teach the desktop app a kind of data kept on Entries. The first is the Card Plugin (payment cards). Third parties can write Plugins from the start. keefast's own Plugins use the same API and ship bundled and signed, so keefast never has a private shortcut that outside authors lack. TOTP predates this and stays in `core`; porting it later is the API's hardest test.

A Plugin is a signed package: a manifest (id, version, publisher key, API version, Plugin capabilities, contributions), a WebAssembly component, and icons. It is installed from a file. Nothing is fetched remotely, as ADR 0008 requires.

- **Code runs in the Rust process, sandboxed in wasmtime** behind WIT interfaces (the WASM component model), one instance per Plugin. Each instance has a memory cap and a CPU limit, and has no filesystem, network or clock unless a capability grants one. Plugins never see the Workspace. A hook gets the values of its Owned fields on one Entry and returns values for the host to write, or text for the host to show. The Workspace stays the only owner of decrypted Vaults, and copying stays Rust-to-clipboard.
- **UI is declarative.** A Plugin declares what it contributes, and the UI draws it with keefast's own kit:
  - a facet on any Entry, with a detection rule and typed Owned fields;
  - sections in the detail and in the editor;
  - Entry list subtitles;
  - command palette commands;
  - later, browser fill mappings.

  Behaviour comes from pure WASM hooks (`format`, `validate`, `derive`). No Plugin JavaScript runs in the webview, so the CSP stays `script-src 'self'; frame-src 'none'`. Sandboxed iframes were rejected for v1: they would put third-party script beside the IPC bridge and need a postMessage protocol to secure. The cost is expressiveness: a Plugin can't draw custom visuals, such as the card-face variant of the card prototype (branch `prototype/credit-card-fields`).
- **Data is plain custom fields** with readable names (`Card number`, `Cardholder`, `Expiry`, `CVV`, `PIN`), detected by name as TOTP's are. A Vault file stays fully usable in KeePassXC, KeePass2 and KeeWeb, and with the Plugin removed (ADR 0002). Installing a Plugin whose Owned fields clash with an installed one's is refused. CustomData markers and CustomData-only storage were rejected: the first adds little, and the second locks data into keefast.
- **Plugin capabilities are granted at install**, and asked for again when an update wants more:
  - the Owned fields;
  - revealing protected values;
  - copying;
  - the clock;
  - browser fill of given form kinds.

  v1 grants no network or filesystem access to any Plugin. Signatures are checked at every load. An unknown publisher's key is shown as unverified and trusted on first use.

## Consequences

- **ADR 0004's threat model gains an attacker: a malicious Plugin the user installed.** Its ways out are limited to what the UI shows, what the host copies, and browser fill, which follows the same Access decisions and Default access policy as logins. Same-user malware stays out of scope; it could replace Plugin files anyway.
- **ADR 0008's rule that secrets cross only when shown gets one exception, for derived labels.** A Plugin with the capability to read its protected Owned fields may have `derive` turn them into a short label the UI shows without a reveal. For example, the Card Plugin keeps `Card number` protected, like `CVV` and `PIN`, and still shows "Visa •••• 1111" in the list and detail. A label is text on screen, never the value itself. A Plugin that put the whole number in it would only show it, not leak it, since it has no network.
- **The mock UI runs the same `.wasm` through `jco`**, so `pnpm dev:mock` shows Plugins without the Rust side.
